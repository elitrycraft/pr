# Improvements to Apply Back to pr.oo.or.id

Bugs and issues discovered while building `rs.oo.or.id` on top of the `pr` engine.

---

## 1. Hardcoded Package Name in Bind Mounts (Critical)

**File:** [`shared.rs`](file:///home/o/Documents/linux-on-android/pr.oo.or.id/src/pr-cli/src/shared.rs#L176-L187)

**Problem:** `build_proot_args()` hardcodes `/data/data/id.or.oo.pr/cache` and `/data/data/id.or.oo.pr` as bind mount paths. Any app with a different package name (like `id.or.oo.rs`) gets `Permission denied` warnings and the bind mounts silently fail.

**Fix applied:** Derive `app_dir` dynamically from `APP_PREFIX`:
```rust
let app_dir = Path::new(&prefix).parent().unwrap().parent().unwrap();
args.push(format!("--bind={}/cache", app_dir.display()));
args.push(format!("--bind={}", app_dir.display()));
```

**Impact:** This also fixes `.l2s` symlink resolution — Alpine's `apk add` creates hard links via proot's `link2symlink` mechanism, which stores absolute symlinks pointing into the app's `.l2s/` directory. Without the correct bind mount, binaries like `gcc` and `cc` become invisible to the guest shell (`/bin/sh: gcc: not found`) even though `ls -l` shows them.

> [!IMPORTANT]
> This is the root cause of the **"linker `cc` not found"** error when running `rustc` on Alpine. It's documented in AGENTS.md as an Alpine-specific test failure, but it's actually a bind mount bug.

---

## 2. `clap` Argument Parsing for `login` Command (Critical)

**File:** [`main.rs`](file:///home/o/Documents/linux-on-android/pr.oo.or.id/src/pr-cli/src/main.rs)

**Problem:** The `Commands::Login` variant used `#[arg(last = true)]` for trailing arguments. This caused `clap` to reject any trailing argument that looked like a flag (e.g. `/bin/sh -c "..."` would fail with `"unexpected argument '-c'"`).

**Fix applied:**
```rust
#[arg(trailing_var_arg = true, allow_hyphen_values = true)]
```

---

## 3. `touch` Silently Fails Inside proot (Important)

**Problem:** BusyBox `touch` uses `utimensat` which proot intercepts. On Alpine inside proot, `touch /etc/.rust_setup` returns exit code 0 but **the file is never created on disk**. The file appears to exist within the same proot session (cached in proot's translation layer) but doesn't persist to the host filesystem.

**Workaround:** Use `echo > /etc/.rust_setup` instead of `touch`. Shell redirection (`>`) uses `open(O_CREAT|O_WRONLY|O_TRUNC)` which proot handles correctly.

> [!WARNING]
> This affects any script that uses `touch` to create marker/sentinel files inside proot. Any existing setup scripts in `pr` that rely on `touch` for similar purposes should be audited.

---

## 4. Double Shell Wrapping via `pr-cli login -- /bin/sh -c "..."` (Important)

**Problem:** `pr-cli login <distro> -- <args>` internally prepends `/bin/sh -l -c "<args.join(' ')>"`. If you explicitly pass `/bin/sh -c "command"` as trailing args, the command gets double-wrapped:

```
/bin/sh -l -c "/bin/sh -c command"
```

The inner quotes get stripped during the `.join(' ')`, causing the inner shell to receive broken arguments.

**Rule:** Never pass `/bin/sh -c` as trailing args to `pr-cli login`. Pass the raw command string directly:
```
# WRONG:
pr-cli login alpine -- /bin/sh -c "apk update && apk add rust"

# CORRECT:
pr-cli login alpine -- "apk update && apk add rust"
```

> [!NOTE]
> This is already documented in AGENTS.md's "pr-cli Inner Shell Parsing" section, but it's easy to forget when building new tooling on top of `pr-cli`.

---

## 5. OCI Container Path vs Legacy Plugin Path (Important)

**Problem:** When installing via direct OCI reference (`docker.io/library/alpine:latest`), rootfs lands in:
```
var/lib/pr/containers/<alias>/rootfs/
```

But legacy plugin installs (`easycli.sh`) used:
```
var/lib/pr/installed-rootfs/<distro>/
```

Any code checking for distro existence must use the correct path for the installation method used.

---

## Summary Table

| # | Issue | Severity | Fixed In | Needs Backport to pr? |
|---|-------|----------|----------|----------------------|
| 1 | Hardcoded package name in bind mounts | Critical | `shared.rs` | ✅ **Already applied** |
| 2 | `clap` rejects hyphen-prefixed trailing args | Critical | `main.rs` | ✅ **Already applied** |
| 3 | `touch` silently fails in proot | Important | Workaround only | ⚠️ Document/audit |
| 4 | Double shell wrapping | Important | Caller-side | ⚠️ Document |
| 5 | OCI vs legacy rootfs paths | Important | Caller-side | ⚠️ Document |

## 6. Prevent `TerminalActivity` Recreation on Keyboard Pop-up (Critical)

**File:** [`AndroidManifest.xml`](file:///home/o/Documents/linux-on-android/pr.oo.or.id/android/app/src/main/AndroidManifest.xml)

**Problem:** `TerminalActivity` lacks `android:configChanges`. By default, Android destroys and recreates the activity when the soft keyboard pops up (which changes the screen size). This destroys the Jetpack Compose `Terminal` state and detaches the PTY session.

**Fix to apply:**
```xml
<activity
    android:name=".TerminalActivity"
    android:exported="false"
    android:configChanges="orientation|screenSize|keyboard|keyboardHidden"
    android:windowSoftInputMode="adjustResize" />
```

---

## 7. Pass Terminal Resizes to PTY Session (Critical)

**File:** [`TerminalActivity.kt`](file:///home/o/Documents/linux-on-android/pr.oo.or.id/android/app/src/main/java/id/or/oo/pr/TerminalActivity.kt)

**Problem:** When the Android soft keyboard opens, Jetpack Compose resizes the `Terminal` component. However, this resize event is not passed to `ProotLauncher.Session`. Applications inside the proot container (like `nano`, `htop`, or even the shell itself) will assume the terminal is permanently stuck at `24x80`.

**Fix to apply:**
```kotlin
val em = TerminalEmulatorFactory.create(
    // ...
    onResize = { dims ->
        session?.resize(dims.rows, dims.columns)
    }
)
```

---

## 8. Prevent Text Occlusion Behind System Status Bar (Important)

**Problem:** If the application uses edge-to-edge drawing (or hides the action bar), the top rows of the Jetpack Compose `Terminal` Canvas will render directly underneath the physical device's camera notch and the Android status bar. The first few rows of output will be completely invisible.

**Fix to apply:** Apply `Modifier.systemBarsPadding()` to the `Terminal` or its parent `Surface`.
```kotlin
Terminal(
    modifier = Modifier.fillMaxSize().systemBarsPadding(),
    // ...
)
```

---

## 9. Robust Argument Passing in `ProotLauncher` (Important)

**File:** [`ProotLauncher.kt`](file:///home/o/Documents/linux-on-android/pr.oo.or.id/android/app/src/main/java/id/or/oo/pr/ProotLauncher.kt)

**Problem:** `runCommand(command: String)` uses `command.split(" ").toTypedArray()` to build the argument list for `forkPty`. This is highly brittle and will shatter quoted arguments (e.g., `bash -c "echo hello"` becomes `["bash", "-c", "\"echo", "hello\""]`).

**Fix to apply:** Add a `startCustomSession` method that accepts a pre-parsed `List<String>` of arguments, identical to what was implemented in the `rs.oo.or.id` fork:
```kotlin
fun startCustomSession(
    args: List<String>,
    rows: Int = 24,
    cols: Int = 80,
): Session? {
    val envVars = buildEnvVars()
    val masterFd = PtyNative.forkPty(args[0], args.toTypedArray(), envVars, rows, cols)
    // ...
}
```
