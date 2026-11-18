//! JSON status of ESDM servers with different entropy sources and settings,
//! and the error paths of a missing, hanging, stopped or restarted server
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
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rand_core::TryRng;
use rand_esdm::{
    EsdmConnection, EsdmDrngType, EsdmEntropySourceStatus, EsdmNotification, EsdmPrivConnection,
    EsdmRng, EsdmRngType, EsdmSelftestState, EsdmStatus, esdm_is_fully_seeded,
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
    /// arguments of the server, `None` to start the case without one
    server_args: Option<&'static [&'static str]>,
    /// serve an external entropy source on [`PROVIDER_SOCKET`] before the
    /// server is started
    provider: bool,
    check: fn(&mut Server),
}

const CASES: &[Case] = &[
    Case {
        name: "default",
        server_args: Some(&[]),
        provider: false,
        check: check_default,
    },
    Case {
        name: "small_memory",
        server_args: Some(&["--small_memory"]),
        provider: false,
        check: check_small_memory,
    },
    Case {
        name: "single_jent_instance",
        server_args: Some(&["--es_jent_instances", "1"]),
        provider: false,
        check: check_single_jent_instance,
    },
    Case {
        name: "es_ext_unconnected",
        server_args: Some(&[
            "--es_ext",
            "/run/rand-esdm-test-absent.socket",
            "--es_ext_entropy_rate",
            "128",
            "--es_ext_uid",
            "1000",
        ]),
        provider: false,
        check: check_es_ext_unconnected,
    },
    Case {
        name: "es_ext_provider_rate",
        server_args: Some(&["--es_ext", PROVIDER_SOCKET, "--es_ext_entropy_rate", "auto"]),
        provider: true,
        check: check_es_ext_provider_rate,
    },
    Case {
        name: "es_ext_configured_rate",
        server_args: Some(&["--es_ext", PROVIDER_SOCKET, "--es_ext_entropy_rate", "32"]),
        provider: true,
        check: check_es_ext_configured_rate,
    },
    Case {
        name: "runtime_changes",
        server_args: Some(&["--small_memory"]),
        provider: false,
        check: check_runtime_changes,
    },
    Case {
        name: "no_server",
        server_args: None,
        provider: false,
        check: check_no_server,
    },
    Case {
        name: "server_hangs",
        server_args: Some(&["--small_memory"]),
        provider: false,
        check: check_server_hangs,
    },
    Case {
        name: "server_restart",
        server_args: Some(&["--small_memory"]),
        provider: false,
        check: check_server_restart,
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

fn check_default(_: &mut Server) {
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

fn check_small_memory(_: &mut Server) {
    let conn = EsdmConnection::new();
    let status = fetch_status(&conn);
    assert_eq!(status.drng_instances, 1);
    // the only DRNG instance is the initial one, serving node 0
    let node0 = conn.drng_status(0).unwrap();
    assert_eq!(node0.drng_type, EsdmDrngType::Initial);
    assert_eq!(node0.node, Some(0));
    assert_eq!(Some(&node0), status.drngs.first());
}

fn check_single_jent_instance(_: &mut Server) {
    let conn = EsdmConnection::new();
    // the Jitter RNG alone still seeds the ESDM, see `check_common`
    fetch_status(&conn);
}

fn check_es_ext_unconnected(_: &mut Server) {
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

fn check_es_ext_provider_rate(_: &mut Server) {
    let conn = EsdmConnection::new();
    let status = wait_for_provider(&conn);
    check_common(&status);
    let ext = source(&status, "ExternalSocket");
    check_provider_source(ext);
    assert_eq!(detail_str(ext, "assessed_rate_source"), "provider");
    assert_eq!(detail(ext, "assessed_rate"), PROVIDER_RATE);
}

fn check_es_ext_configured_rate(_: &mut Server) {
    let conn = EsdmConnection::new();
    let status = wait_for_provider(&conn);
    check_common(&status);
    let ext = source(&status, "ExternalSocket");
    check_provider_source(ext);
    // the configured rate wins over the one the provider declares
    assert_eq!(detail_str(ext, "assessed_rate_source"), "config");
    assert_eq!(detail(ext, "assessed_rate"), 32);
}

fn check_runtime_changes(_: &mut Server) {
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

/// asserts that `f` fails with one of `kinds` within `max`
fn assert_fails<T: std::fmt::Debug>(
    what: &str,
    kinds: &[ErrorKind],
    max: Duration,
    f: impl FnOnce() -> std::io::Result<T>,
) {
    let start = Instant::now();
    let res = f();
    let elapsed = start.elapsed();
    match res {
        Err(e) => assert!(kinds.contains(&e.kind()), "{what}: unexpected error {e:?}"),
        Ok(v) => panic!("{what}: unexpected success {v:?}"),
    }
    assert!(elapsed < max, "{what}: took {elapsed:?}");
}

/// every kind of call fails with one of `kinds` within `max`
fn assert_all_fail(conn: &EsdmConnection, rng: &mut EsdmRng, kinds: &[ErrorKind], max: Duration) {
    assert_fails("status", kinds, max, || conn.status());
    assert_fails("status text", kinds, max, || conn.status_text());
    assert_fails("entropy count", kinds, max, || conn.entropy_count());
    assert_fails("write data", kinds, max, || conn.write_data(&[0x5a; 8]));
    assert_fails("random bytes", kinds, max, || {
        rng.try_fill_bytes(&mut [0u8; 32])
    });
    assert_fails("privileged call", kinds, max, || {
        EsdmPrivConnection::try_new()?.crng_reseed()
    });
    assert_eq!(esdm_is_fully_seeded(), None);
}

/// every kind of call works
fn assert_all_work(conn: &EsdmConnection, rng: &mut EsdmRng) {
    assert!(conn.status().unwrap().fully_seeded);
    conn.entropy_count().unwrap();
    conn.write_data(&[0x5a; 8]).unwrap();
    rng.try_fill_bytes(&mut [0u8; 32]).unwrap();
    EsdmPrivConnection::new().crng_reseed().unwrap();
    assert_eq!(esdm_is_fully_seeded(), Some(true));
}

fn check_no_server(_: &mut Server) {
    // the client library connects lazily, so setting up works
    let conn = EsdmConnection::try_new().unwrap();
    let mut rng = EsdmRng::try_new(EsdmRngType::FullySeeded).unwrap();

    // the socket is missing: the calls fail right away and say why
    assert_all_fail(
        &conn,
        &mut rng,
        &[ErrorKind::NotFound],
        Duration::from_secs(1),
    );

    // nobody signals the need for entropy, so the wait times out on time
    let mut notification = EsdmNotification::try_new().unwrap();
    let start = Instant::now();
    assert_fails(
        "notification",
        &[ErrorKind::TimedOut],
        Duration::from_secs(5),
        || notification.wait_for_entropy_needed_timeout(Duration::from_millis(300)),
    );
    assert!(start.elapsed() >= Duration::from_millis(300));
}

fn check_server_restart(server: &mut Server) {
    let conn = EsdmConnection::new();
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
    assert_all_work(&conn, &mut rng);

    // A stopped server is reported as such. The client library tries to
    // reconnect for a while before it gives up, the retries of this crate
    // must not multiply that.
    server.stop();
    assert_all_fail(
        &conn,
        &mut rng,
        &[ErrorKind::ConnectionRefused, ErrorKind::NotFound],
        Duration::from_secs(10),
    );

    // the same connection and RNG work again once the server is back
    server.start();
    server.wait_until_seeded();
    assert_all_work(&conn, &mut rng);
}

fn check_server_hangs(server: &mut Server) {
    let conn = EsdmConnection::new();
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
    assert_all_work(&conn, &mut rng);

    // A server which does not answer is reported after the timeout of the
    // client library (a few seconds). The retries of this crate must not
    // multiply it.
    server.signal(libc::SIGSTOP);
    let max = Duration::from_secs(10);
    assert_fails("status", &[ErrorKind::TimedOut], max, || conn.status());
    assert_fails("entropy count", &[ErrorKind::TimedOut], max, || {
        conn.entropy_count()
    });
    assert_fails("random bytes", &[ErrorKind::TimedOut], max, || {
        rng.try_fill_bytes(&mut [0u8; 32])
    });

    // waiting for a request for entropy does not talk to the server, so its
    // timeout bounds it also now
    let mut notification = EsdmNotification::try_new().unwrap();
    let start = Instant::now();
    match notification.wait_for_entropy_needed_timeout(Duration::from_millis(300)) {
        Ok(()) => {}
        Err(e) => assert_eq!(e.kind(), ErrorKind::TimedOut, "{e:?}"),
    }
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );

    // and everything works again once it answers
    server.signal(libc::SIGCONT);
    assert_all_work(&conn, &mut rng);
}

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

/// the `esdm-server` of a case, which a check may stop and start again
struct Server {
    path: PathBuf,
    args: &'static [&'static str],
    child: Option<Child>,
}

impl Server {
    fn start(&mut self) {
        assert!(self.child.is_none(), "server already running");
        // appends, so the log of a restarted server keeps the earlier runs
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(SERVER_LOG)
            .unwrap();
        let child = Command::new(&self.path)
            .arg("--foreground")
            .args(self.args)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap_or_else(|e| panic!("cannot start {}: {e}", self.path.display()));
        self.child = Some(child);
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn signal(&self, sig: c_int) {
        let child = self.child.as_ref().expect("server not running");
        let pid = libc::pid_t::try_from(child.id()).unwrap();
        // SAFETY: plain syscall on the pid of our own child, which is not
        // reaped yet, so the pid cannot have been reused.
        assert_eq!(unsafe { libc::kill(pid, sig) }, 0);
    }

    fn wait_until_seeded(&mut self) {
        wait_for("the ESDM to be fully seeded", || {
            if let Some(Ok(Some(status))) = self.child.as_mut().map(Child::try_wait) {
                panic!("esdm-server exited with {status}");
            }
            esdm_is_fully_seeded().filter(|&seeded| seeded)
        });
    }
}

fn run_inside(case: &Case) -> ExitCode {
    let server = PathBuf::from(env::var_os(SERVER_ENV).expect("ESDM_SERVER not set"));

    mount_tmpfs(c"/run");
    mount_tmpfs(c"/dev/shm");

    if case.provider {
        serve_provider();
    }
    let mut server = Server {
        path: server,
        args: case.server_args.unwrap_or_default(),
        child: None,
    };

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        if case.server_args.is_some() {
            server.start();
            server.wait_until_seeded();
        }
        (case.check)(&mut server);
    }));

    server.stop();

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
