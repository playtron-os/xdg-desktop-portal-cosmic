//! Which workspace a request came from, and where that workspace's files are.
//!
//! An app inside a kora workspace reaches this portal through its
//! workspace's router, so the caller the frontend sees — named in the request
//! handle it hands us — is a process in that workspace's slice. From there:
//! its pid, its cgroup, the workspace id, and the registry's answer for where
//! the workspace's directories are on the machine, by the same variable names
//! the workspace's own apps are told. A capture is then saved where the app
//! that asked for it will find it, whether its workspace is on screen or not;
//! the router hands it the path it sees. A machine-plane caller resolves to no
//! workspace and gets this session's directories, as before.

use std::collections::HashMap;
use zbus::names::BusName;
use zbus::zvariant::ObjectPath;

const REGISTRY: &str = "one.playtron.Workspaces1";
const OBJECT: &str = "/one/playtron/Workspaces1";

/// The request handle's shape: the caller's unique name, `:` dropped and
/// `.` as `_`, then the token.
const REQUEST_PREFIX: &str = "/org/freedesktop/portal/desktop/request/";

/// `HOME`, the XDG base and user dirs and `XDG_SCREENSHOTS_DIR` of the
/// workspace the request in `handle` came from, if it came from one and a
/// registry answers.
pub async fn caller_dirs(
    connection: &zbus::Connection,
    handle: &ObjectPath<'_>,
) -> Option<HashMap<String, String>> {
    let caller = caller_of(handle)?;
    let pid = zbus::fdo::DBusProxy::new(connection)
        .await
        .ok()?
        .get_connection_unix_process_id(BusName::try_from(caller).ok()?)
        .await
        .ok()?;
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let workspace = workspace_of_cgroup(&cgroup)?;
    let registry = zbus::Proxy::new(connection, REGISTRY, OBJECT, REGISTRY)
        .await
        .ok()?;
    match registry.call("Dirs", &(workspace.as_str(),)).await {
        Ok(dirs) => Some(dirs),
        // A slice the registry does not know: a workspace deleted while its
        // sandbox still runs. Said, not swallowed, before the fallback to
        // whatever is on screen.
        Err(error) => {
            tracing::warn!(%workspace, %error, "the caller's workspace is not one the registry knows");
            None
        }
    }
}

/// The directories of the workspace on screen, when a registry answers. The
/// fallback for a caller that is not itself in a workspace — a screenshot
/// triggered by a compositor shortcut runs a machine-plane tool, but the
/// capture is of whatever workspace is on screen, so it belongs to that one.
pub async fn active_dirs(connection: &zbus::Connection) -> Option<HashMap<String, String>> {
    let registry = zbus::Proxy::new(connection, REGISTRY, OBJECT, REGISTRY)
        .await
        .ok()?;
    // An empty id means the active workspace.
    registry.call("Dirs", &("",)).await.ok()
}

/// The unique name a request handle was made for.
fn caller_of(handle: &ObjectPath<'_>) -> Option<String> {
    let sender = handle
        .as_str()
        .strip_prefix(REQUEST_PREFIX)?
        .split('/')
        .next()
        .filter(|s| !s.is_empty())?;
    Some(format!(":{}", sender.replace('_', ".")))
}

/// The workspace a cgroup file names, or `None` for the machine plane. The
/// deepest `workspace-<id>.slice`: systemd nests `workspace-a-b.slice` inside
/// `workspace-a.slice`, so the first is a prefix, not the id. The same rule as
/// kora-workspaces' `workspace-caller` crate, which every gate on the machine
/// uses; kept in step with it.
fn workspace_of_cgroup(cgroup: &str) -> Option<String> {
    cgroup
        .split(['/', ':', '\n'])
        .filter_map(|segment| {
            segment
                .strip_prefix("workspace-")
                .and_then(|rest| rest.strip_suffix(".slice"))
        })
        .next_back()
        .filter(|id| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caller_is_read_back_out_of_the_request_handle() {
        let handle =
            ObjectPath::try_from("/org/freedesktop/portal/desktop/request/1_197/kora42").unwrap();
        assert_eq!(caller_of(&handle), Some(":1.197".to_string()));
        let bare = ObjectPath::try_from("/org/freedesktop/portal/desktop").unwrap();
        assert_eq!(caller_of(&bare), None);
    }

    #[test]
    fn the_workspace_is_the_deepest_slice_and_the_machine_plane_is_none() {
        assert_eq!(
            workspace_of_cgroup(
                "0::/user.slice/user-1000.slice/user@1000.service/workspace.slice/workspace-meridian.slice/run-r1.scope\n"
            ),
            Some("meridian".to_string())
        );
        assert_eq!(
            workspace_of_cgroup(
                "0::/user.slice/user@1000.service/workspace.slice/workspace-a.slice/workspace-a-b.slice/x.scope\n"
            ),
            Some("a-b".to_string())
        );
        assert_eq!(
            workspace_of_cgroup(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-foo.scope\n"
            ),
            None
        );
    }
}
