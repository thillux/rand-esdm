//! JSON status of ESDM servers with different entropy sources and settings
//!
//! Every case runs an `esdm-server` of its own in a private user, mount, IPC
//! and PID namespace with fresh tmpfs mounts on `/run` and `/dev/shm`, so the
//! server is reachable at its usual socket paths without root and without
//! touching an ESDM server running on the host.
//!
//! Requirements: `unshare` from util-linux >= 2.38 and `newuidmap`/`newgidmap`
//! with a subordinate id range of the user (`/etc/subuid`, `/etc/subgid`),
//! needed by `--map-auto` so the server can switch to `nobody` and drop its
//! supplementary groups. The server is taken from `$ESDM_SERVER` or `$PATH`.
//!
//! Run with `cargo test -p rand-esdm --features sandbox_tests --test
//! status_sandbox [-- <filter>...]`.

use std::env;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::fs::File;
use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rand_core::TryRng;
use rand_esdm::{
    EsdmConnection, EsdmDrngType, EsdmEntropySourceStatus, EsdmPrivConnection, EsdmRng,
    EsdmRngType, EsdmSelftestState, EsdmStatus, esdm_is_fully_seeded,
};
use serde_json::{Value, json};

/// environment variable selecting the case to run inside the sandbox
const CASE_ENV: &str = "RAND_ESDM_SANDBOX_CASE";
/// environment variable naming the `esdm-server` binary
const SERVER_ENV: &str = "ESDM_SERVER";

const SERVER_LOG: &str = "/run/rand-esdm-test-server.log";
const PROVIDER_SOCKET: &str = "/run/rand-esdm-test-provider.socket";
const PROVIDER_NAME: &str = "rand-esdm-test";

const TIMEOUT: Duration = Duration::from_secs(60);

struct Case {
    name: &'static str,
    server_args: &'static [&'static str],
    /// serve an external entropy source on [`PROVIDER_SOCKET`] before the
    /// server is started
    provider: bool,
    check: fn(),
}

const CASES: &[Case] = &[
    Case {
        name: "default",
        server_args: &[],
        provider: false,
        check: check_default,
    },
    Case {
        name: "small_memory",
        server_args: &["--small_memory"],
        provider: false,
        check: check_small_memory,
    },
    Case {
        name: "single_jent_instance",
        server_args: &["--es_jent_instances", "1"],
        provider: false,
        check: check_single_jent_instance,
    },
    Case {
        name: "es_ext_unconnected",
        server_args: &[
            "--es_ext",
            "/run/rand-esdm-test-absent.socket",
            "--es_ext_entropy_rate",
            "128",
            "--es_ext_uid",
            "1000",
        ],
        provider: false,
        check: check_es_ext_unconnected,
    },
    Case {
        name: "es_ext_provider_rate",
        server_args: &["--es_ext", PROVIDER_SOCKET, "--es_ext_entropy_rate", "auto"],
        provider: true,
        check: check_es_ext_provider_rate,
    },
    Case {
        name: "es_ext_configured_rate",
        server_args: &["--es_ext", PROVIDER_SOCKET, "--es_ext_entropy_rate", "32"],
        provider: true,
        check: check_es_ext_configured_rate,
    },
    Case {
        name: "runtime_changes",
        server_args: &["--small_memory"],
        provider: false,
        check: check_runtime_changes,
    },
];

/*
 * helpers for the checks
 */

/// polls `f` until it returns `Some`, panics after [`TIMEOUT`]
fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(200));
    }
}

/// the entropy source with the given name (or instance name, for the
/// external socket source which takes the name of its provider)
fn source<'a>(status: &'a EsdmStatus, name: &str) -> &'a EsdmEntropySourceStatus {
    status
        .entropy_sources
        .iter()
        .find(|es| es.name == name || es.details.get("instance") == Some(&json!(name)))
        .unwrap_or_else(|| panic!("no entropy source {name} in {status:#?}"))
}

fn detail<'a>(es: &'a EsdmEntropySourceStatus, key: &str) -> &'a Value {
    es.details
        .get(key)
        .unwrap_or_else(|| panic!("entropy source {} lacks {key}: {es:#?}", es.name))
}

fn detail_str<'a>(es: &'a EsdmEntropySourceStatus, key: &str) -> &'a str {
    detail(es, key)
        .as_str()
        .unwrap_or_else(|| panic!("{key} of entropy source {} is no string", es.name))
}

/// configured online CPUs, the upper bound of DRNG instances
fn cpus() -> u32 {
    // SAFETY: sysconf only reads system configuration.
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) };
    u32::try_from(n).expect("number of CPUs")
}

/// checks what every status document has to satisfy, whatever the
/// configuration
fn check_common(status: &EsdmStatus) {
    assert!(status.library_version.starts_with("1."), "{status:#?}");
    assert_ne!(status.drng_name, "");
    assert!(status.security_strength_bits >= 256);
    assert!(status.fully_seeded);
    assert!(status.drng_instances >= 1 && status.drng_instances <= cpus());

    // ids enumerate the sources
    for (i, es) in status.entropy_sources.iter().enumerate() {
        assert_eq!(usize::try_from(es.id).unwrap(), i, "{es:#?}");
        assert_ne!(es.name, "");
        assert!(!es.details.contains_key("id") && !es.details.contains_key("name"));
    }

    // sources present in every build
    let jent = source(status, "JitterRNG");
    assert!(jent.available_entropy.is_some_and(|e| e > 0), "{jent:#?}");
    assert!(jent.entropy_rate_per_256_data_bits.is_some_and(|r| r > 0));
    assert!(detail(jent, "library_version").is_u64());
    let aux = source(status, "Auxiliary");
    assert!(aux.available_entropy.is_some());
    assert!(detail(aux, "maximum_entropy").as_u64().unwrap() > 0);
    assert!(detail(aux, "write_wakeup_threshold").is_u64());
    source(status, "ExternalSocket");
    // sources which cannot work report why, not their entropy
    for es in &status.entropy_sources {
        if let Some(reason) = es.details.get("disabled") {
            assert!(reason.is_string());
            assert_eq!(es.available_entropy, None);
        }
    }

    // the initial and the prediction resistant DRNG
    let types: Vec<_> = status.drngs.iter().map(|d| d.drng_type).collect();
    assert_eq!(
        types,
        [EsdmDrngType::Initial, EsdmDrngType::PredictionResistance]
    );
    let init = &status.drngs[0];
    assert!(init.fully_seeded && init.initiated);
    assert!(init.seed_generation >= 1);
    assert!(init.name.is_some());
    assert!(is_utc_timestamp(init.last_seeding_time.as_deref().unwrap()));
    assert!(init.seconds_since_last_reseed.is_some_and(|s| s >= 0));
    let pr = &status.drngs[1];
    assert_eq!(pr.node, None);

    assert!(status.reseed_worker.running);
    assert!(status.reseed_worker.reseed_interval_seconds > 0);

    let st = &status.periodic_self_test;
    assert_eq!(st.state, EsdmSelftestState::Passed);
    assert_eq!(st.entropy_source_state, EsdmSelftestState::Passed);
    assert!(st.completed_self_tests >= 1);
    assert_eq!(st.entropy_sources_failed, 0);
    assert_eq!(
        usize::try_from(st.entropy_sources_tested).unwrap(),
        status.entropy_sources.len()
    );
}

/// `YYYY-MM-DDTHH:MM:SSZ`
fn is_utc_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(i, &c)| match i {
            4 | 7 => c == b'-',
            10 => c == b'T',
            13 | 16 => c == b':',
            19 => c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

fn fetch_status(conn: &EsdmConnection) -> EsdmStatus {
    let status = conn.status().expect("status");
    check_common(&status);
    status
}

/// waits until the external socket source is connected to its provider
fn wait_for_provider(conn: &EsdmConnection) -> EsdmStatus {
    wait_for("connection to the entropy source provider", || {
        let status = conn.status().ok()?;
        detail_str(source(&status, "ExternalSocket"), "status")
            .starts_with("connected")
            .then_some(status)
    })
}

/*
 * checks of the cases, run inside the sandbox
 */

fn check_default() {
    let conn = EsdmConnection::new();
    let status = fetch_status(&conn);

    let ext = source(&status, "ExternalSocket");
    assert_eq!(ext.name, "ExternalSocket");
    assert_eq!(detail_str(ext, "status"), "not connected");
    assert_eq!(detail_str(ext, "provider_socket"), "(unset)");
    assert_eq!(detail_str(ext, "provider_accepted"), "root");
    assert_eq!(detail_str(ext, "assessed_rate_source"), "default");
    assert_eq!(detail(ext, "assessed_rate"), 0);
    assert_eq!(ext.available_entropy, Some(0));
    assert!(!ext.details.contains_key("provider"));

    // the single DRNG documents match their entries of the status
    let pr = conn.drng_status_pr().unwrap();
    assert_eq!(pr.drng_type, EsdmDrngType::PredictionResistance);
    assert_eq!(pr.node, None);
    let node0 = conn.drng_status(0).unwrap();
    assert!(matches!(
        node0.drng_type,
        EsdmDrngType::Initial | EsdmDrngType::Node
    ));
    assert_eq!(node0.node, Some(0));
    assert!(node0.fully_seeded);
}

fn check_small_memory() {
    let conn = EsdmConnection::new();
    let status = fetch_status(&conn);
    assert_eq!(status.drng_instances, 1);
    // the only DRNG instance is the initial one, serving node 0
    let node0 = conn.drng_status(0).unwrap();
    assert_eq!(node0.drng_type, EsdmDrngType::Initial);
    assert_eq!(node0.node, Some(0));
    assert_eq!(Some(&node0), status.drngs.first());
}

fn check_single_jent_instance() {
    let conn = EsdmConnection::new();
    // the Jitter RNG alone still seeds the ESDM, see `check_common`
    fetch_status(&conn);
}

fn check_es_ext_unconnected() {
    let conn = EsdmConnection::new();
    let status = fetch_status(&conn);
    let ext = source(&status, "ExternalSocket");
    assert_eq!(ext.name, "ExternalSocket");
    assert_eq!(detail_str(ext, "status"), "not connected");
    assert_eq!(
        detail_str(ext, "provider_socket"),
        "/run/rand-esdm-test-absent.socket"
    );
    assert_eq!(detail_str(ext, "provider_accepted"), "root and uid 1000");
    assert_eq!(detail_str(ext, "assessed_rate_source"), "config");
    assert_eq!(detail(ext, "assessed_rate"), 128);
    // nothing is credited without a provider
    assert_eq!(ext.available_entropy, Some(0));
    assert_eq!(ext.entropy_rate_per_256_data_bits, Some(0));
}

/// checks the external socket source connected to the provider of
/// [`serve_provider`]
fn check_provider_source(ext: &EsdmEntropySourceStatus) {
    assert_eq!(ext.name, PROVIDER_NAME);
    assert_eq!(detail_str(ext, "instance"), "ExternalSocket");
    assert_eq!(detail_str(ext, "provider_socket"), PROVIDER_SOCKET);

    let provider = detail(ext, "provider");
    assert_eq!(provider["plugin"], json!({ "source": "rand-esdm test" }));
    assert_eq!(provider["server"]["name"], PROVIDER_NAME);
    assert_eq!(provider["server"]["entropy_rate"], PROVIDER_RATE);
}

fn check_es_ext_provider_rate() {
    let conn = EsdmConnection::new();
    let status = wait_for_provider(&conn);
    check_common(&status);
    let ext = source(&status, "ExternalSocket");
    check_provider_source(ext);
    assert_eq!(detail_str(ext, "assessed_rate_source"), "provider");
    assert_eq!(detail(ext, "assessed_rate"), PROVIDER_RATE);
}

fn check_es_ext_configured_rate() {
    let conn = EsdmConnection::new();
    let status = wait_for_provider(&conn);
    check_common(&status);
    let ext = source(&status, "ExternalSocket");
    check_provider_source(ext);
    // the configured rate wins over the one the provider declares
    assert_eq!(detail_str(ext, "assessed_rate_source"), "config");
    assert_eq!(detail(ext, "assessed_rate"), 32);
}

fn check_runtime_changes() {
    let conn = EsdmConnection::new();
    let priv_conn = EsdmPrivConnection::new();
    let before = fetch_status(&conn);

    // a forced reseed starts a new seed generation of the initial DRNG
    priv_conn.crng_reseed().unwrap();
    let generation = before.drngs[0].seed_generation;
    wait_for("reseed of the initial DRNG", || {
        let node0 = conn.drng_status(0).ok()?;
        (node0.seed_generation > generation).then_some(())
    });

    // entropy written to the auxiliary pool is accounted there
    priv_conn.add_entropy(&[0x5a; 64], 512).unwrap();
    let aux = source(&fetch_status(&conn), "Auxiliary").available_entropy;
    assert!(aux.is_some_and(|e| e > 0), "{aux:?}");

    // a self test run is counted
    let result = priv_conn.selftest().unwrap();
    assert!(result.all_passed(), "{result:?}");
    let completed = fetch_status(&conn).periodic_self_test.completed_self_tests;
    assert!(completed > before.periodic_self_test.completed_self_tests);

    // the prediction resistant DRNG is seeded once it is used
    assert_eq!(before.drngs[1].seed_generation, 0);
    assert_eq!(before.drngs[1].last_seeding_time, None);
    let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);
    let mut buf = [0u8; 32];
    rng.try_fill_bytes(&mut buf).unwrap();
    let pr = conn.drng_status_pr().unwrap();
    assert!(pr.seed_generation >= 1, "{pr:#?}");
    assert!(is_utc_timestamp(pr.last_seeding_time.as_deref().unwrap()));
    assert!(pr.seconds_since_last_reseed.is_some());
}

/*
 * external entropy source provider, served by libesdm_es_ext
 */

const PROVIDER_RATE: u32 = 200;

/// `struct esdm_es_ext_plugin` of `esdm_es_ext.h`
#[repr(C)]
struct EsExtPlugin {
    version: u32,
    name: *const c_char,
    init: Option<unsafe extern "C" fn(*mut *mut c_void) -> c_int>,
    fini: Option<unsafe extern "C" fn(*mut c_void)>,
    get_entropy: Option<unsafe extern "C" fn(*mut c_void, *mut u8, usize, *mut u32) -> c_int>,
    selftest: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    status_json: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> c_int>,
    entropy_rate: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
}

// SAFETY: the plugin is immutable and only points to static data.
unsafe impl Sync for EsExtPlugin {}

/// opaque `struct esdm_es_ext_server`
#[repr(C)]
struct EsExtServer {
    _private: [u8; 0],
}

#[link(name = "esdm_es_ext")]
unsafe extern "C" {
    fn esdm_es_ext_server_alloc(
        plugin: *const EsExtPlugin,
        socket_path: *const c_char,
        mode: libc::mode_t,
        server: *mut *mut EsExtServer,
    ) -> c_int;
    fn esdm_es_ext_server_run(server: *mut EsExtServer) -> c_int;
}

/// fills the buffer from the kernel RNG, crediting every bit
unsafe extern "C" fn provider_get_entropy(
    _ctx: *mut c_void,
    buf: *mut u8,
    buflen: usize,
    entropy_bits: *mut u32,
) -> c_int {
    // SAFETY: libesdm_es_ext hands out `buf` writable for `buflen` bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, buflen) };
    if getrandom::fill(buf).is_err() {
        return -libc::EIO;
    }
    // SAFETY: `entropy_bits` is a writable `uint32_t`.
    unsafe { *entropy_bits = u32::try_from(buflen * 8).unwrap_or(u32::MAX) };
    0
}

unsafe extern "C" fn provider_status_json(
    _ctx: *mut c_void,
    buf: *mut c_char,
    buflen: usize,
) -> c_int {
    const STATUS: &CStr = c"{\"source\":\"rand-esdm test\"}";
    let status = STATUS.to_bytes_with_nul();
    if status.len() > buflen {
        return -libc::EMSGSIZE;
    }
    // SAFETY: `buf` is writable for `buflen` bytes, which `status` fits into.
    unsafe { std::ptr::copy_nonoverlapping(status.as_ptr(), buf.cast::<u8>(), status.len()) };
    0
}

unsafe extern "C" fn provider_entropy_rate(_ctx: *mut c_void) -> u32 {
    PROVIDER_RATE
}

static PROVIDER: EsExtPlugin = EsExtPlugin {
    version: 1,
    name: c"rand-esdm-test".as_ptr(),
    init: None,
    fini: None,
    get_entropy: Some(provider_get_entropy),
    selftest: None,
    status_json: Some(provider_status_json),
    entropy_rate: Some(provider_entropy_rate),
};

/// serves [`PROVIDER`] on [`PROVIDER_SOCKET`] in a thread of its own, for the
/// rest of the life of the process
fn serve_provider() {
    let path = c"/run/rand-esdm-test-provider.socket";
    assert_eq!(path.to_str(), Ok(PROVIDER_SOCKET));

    let mut server = std::ptr::null_mut();
    // SAFETY: `PROVIDER` and `path` are static, `server` is writable.
    let ret = unsafe {
        esdm_es_ext_server_alloc(&raw const PROVIDER, path.as_ptr(), 0o666, &raw mut server)
    };
    assert_eq!(ret, 0, "cannot serve the entropy source provider");

    // the server is used by this thread only from here on
    let server = server as usize;
    thread::spawn(move || {
        // SAFETY: `server` was allocated above and is never freed.
        let ret = unsafe { esdm_es_ext_server_run(server as *mut EsExtServer) };
        panic!("entropy source provider stopped: {ret}");
    });
}

/*
 * inside of the sandbox
 */

fn mount_tmpfs(target: &CStr) {
    // SAFETY: all arguments are NUL-terminated strings or NULL.
    let ret = unsafe {
        libc::mount(
            c"tmpfs".as_ptr(),
            target.as_ptr(),
            c"tmpfs".as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    assert_eq!(
        ret,
        0,
        "cannot mount tmpfs on {target:?}: {}",
        std::io::Error::last_os_error()
    );
}

fn start_server(server: &Path, args: &[&str]) -> Child {
    let log = File::create(SERVER_LOG).unwrap();
    Command::new(server)
        .arg("--foreground")
        .args(args)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start {}: {e}", server.display()))
}

fn run_inside(case: &Case) -> ExitCode {
    let server = PathBuf::from(env::var_os(SERVER_ENV).expect("ESDM_SERVER not set"));

    mount_tmpfs(c"/run");
    mount_tmpfs(c"/dev/shm");

    if case.provider {
        serve_provider();
    }
    let mut child = start_server(&server, case.server_args);

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        wait_for("the ESDM to be fully seeded", || {
            if let Ok(Some(status)) = child.try_wait() {
                panic!("esdm-server exited with {status}");
            }
            esdm_is_fully_seeded().filter(|&seeded| seeded)
        });
        (case.check)();
    }));

    let _ = child.kill();
    let _ = child.wait();

    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        let mut log = String::new();
        let _ = File::open(SERVER_LOG).and_then(|mut f| f.read_to_string(&mut log));
        eprintln!("---- esdm-server output ----\n{log}");
        ExitCode::FAILURE
    }
}

/*
 * outside of the sandbox
 */

/// absolute path of the server, which stays reachable after `/run` (and with
/// it e.g. `/run/current-system` of NixOS) is hidden in the sandbox
fn find_server() -> PathBuf {
    let server = env::var_os(SERVER_ENV).map_or_else(
        || {
            env::split_paths(&env::var_os("PATH").unwrap_or_default())
                .map(|dir| dir.join("esdm-server"))
                .find(|p| p.is_file())
                .expect("esdm-server not found in PATH, set ESDM_SERVER")
        },
        PathBuf::from,
    );
    server
        .canonicalize()
        .unwrap_or_else(|e| panic!("{}: {e}", server.display()))
}

fn run_sandboxed(case: &Case, server: &Path) -> Result<(), String> {
    let output = Command::new("unshare")
        .args([
            "--map-root-user",
            "--map-auto",
            "--mount",
            "--ipc",
            "--pid",
            "--fork",
            "--mount-proc",
            "--",
        ])
        .arg(env::current_exe().unwrap())
        .env(CASE_ENV, case.name)
        .env(SERVER_ENV, server)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run unshare: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}\n---- stdout ----\n{}\n---- stderr ----\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn main() -> ExitCode {
    if let Ok(name) = env::var(CASE_ENV) {
        let case = CASES.iter().find(|c| c.name == name).expect("unknown case");
        return run_inside(case);
    }

    // positional arguments filter the cases, libtest options are ignored
    let filters: Vec<String> = env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|c| filters.is_empty() || filters.iter().any(|f| c.name.contains(f.as_str())))
        .collect();

    let server = find_server();
    println!(
        "\nrunning {} sandboxed tests with {}",
        cases.len(),
        server.display()
    );

    let results: Vec<_> = thread::scope(|s| {
        let handles: Vec<_> = cases
            .iter()
            .map(|case| s.spawn(|| run_sandboxed(case, &server)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut failed = 0;
    for (case, result) in cases.iter().zip(&results) {
        match result {
            Ok(()) => println!("test {} ... ok", case.name),
            Err(err) => {
                failed += 1;
                println!("test {} ... FAILED\n{err}", case.name);
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed\n",
        if failed == 0 { "ok" } else { "FAILED" },
        cases.len() - failed
    );

    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
