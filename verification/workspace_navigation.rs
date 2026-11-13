mod twin;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use twin::{b64_encode, obj, s, Controller, Value};

const SCOPE: &str = "Owner public core-event checks, actual separate filesystem workspaces, real loopback MCP initialization/discovery/review/calls and server artifact, source revocation, and controlled Android document callbacks. Android account/provider views require installed-device execution.";

fn main() {
    let project = std::env::current_dir().unwrap_or_else(|error| panic!("cwd: {error}"));
    let scratch = twin::create_scratch(&project, "workspace-controls");
    let original = scratch.join("workspace");
    let remote = scratch.join("server");
    std::fs::create_dir(&original).unwrap_or_else(|error| panic!("workspace: {error}"));
    std::fs::create_dir(&remote).unwrap_or_else(|error| panic!("server: {error}"));
    std::fs::write(original.join("keep.txt"), "Original saved work\n")
        .unwrap_or_else(|error| panic!("keep.txt: {error}"));
    let project_text = project
        .to_str()
        .unwrap_or_else(|| panic!("project root is not UTF-8"))
        .to_string();
    let passed = {
        let peer = Peer::start(remote, "/mcp", "controlled-token", Some("controlled-session"), true);
        let mut controller = Controller::spawn(&project_text);
        let mut drive = Drive {
            controller: &mut controller,
            base: scratch.clone(),
            original: original.clone(),
            peer: &peer,
        };
        let passed = drive.run();
        controller.finish();
        passed
    };
    if passed.len() != 83 {
        panic!("emitted {} checks, expected 83", passed.len());
    }
    let body = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
    ]);
    twin::report_value("workspace_navigation", &body, true);
    // The leg prints dict(result, count=len(passed)): the written record plus a count key,
    // so the printed channel is built separately from the written body.
    let printed = obj(vec![
        (
            "passed".into(),
            Value::Arr(passed.iter().map(|name| s(name)).collect()),
        ),
        ("scope".into(), s(SCOPE)),
        ("count".into(), twin::n(&passed.len().to_string())),
    ]);
    println!("{}", twin::pretty(&printed));
    let _ = std::fs::remove_dir_all(&scratch);
}

struct Drive<'a> {
    controller: &'a mut Controller,
    base: PathBuf,
    original: PathBuf,
    peer: &'a Peer,
}

impl Drive<'_> {
    fn run(&mut self) -> Vec<String> {
        let mut passed = Vec::new();
        let endpoint = format!("{}/mcp", self.peer.origin());
        let mut d = self.fire(None, "resume", obj(vec![]));
        d = self.fire(Some(state(&d)), "workspace", obj(vec![]));
        let labels = labels(&d);
        let workspace_events = events(&d);
        keep(
            &mut passed,
            "Workspace has six unique categories",
            labels
                == [
                    "Projects",
                    "Storage",
                    "Tools and servers",
                    "Investigation",
                    "Programs",
                    "Setup",
                ]
                && unique(&workspace_events)
                && workspace_events.len() == 6,
        );
        d = self.fire(Some(state(&d)), "workspace_storage", obj(vec![]));
        keep(
            &mut passed,
            "Storage separates phone, network and document provider",
            same(&events(&d), &["phone_locations", "network_locations", "document_providers"]),
        );
        d = self.fire(Some(state(&d)), "phone_locations", obj(vec![]));
        let phone = events(&d);
        keep(
            &mut passed,
            "phone browsing has no connection setup action",
            phone.iter().any(|event| event == "phone_files")
                && !phone.iter().any(|event| event == "phone_folder")
                && !phone.iter().any(|event| event == "phone_access_full"),
        );
        d = self.fire(Some(state(&d)), "close", obj(vec![]));
        keep(
            &mut passed,
            "Back returns from phone to Storage",
            str_at(state(&d), &["dialog"]) == Some("workspace_storage"),
        );
        d = self.fire(Some(state(&d)), "close", obj(vec![]));
        keep(
            &mut passed,
            "Back returns from Storage to Workspace",
            falsy_get(state(&d), "dialog") && str_at(&d, &["ui", "page"]) == Some("workspace"),
        );

        let snapshot = state(&d).clone();
        let mut reachable = Vec::new();
        for section in [
            "workspace_projects",
            "workspace_files",
            "workspace_snapshots",
            "workspace_storage",
            "phone_locations",
            "network_locations",
            "google_drive",
            "workspace_setup",
            "phone_access",
            "workspace_programs",
            "workspace_research",
        ] {
            for event in events(&self.fire(Some(&snapshot), section, obj(vec![]))) {
                if !reachable.iter().any(|item: &String| item == &event) {
                    reachable.push(event);
                }
            }
        }
        let original_actions = [
            "phone_access",
            "drive_list",
            "drive_edit",
            "files",
            "project_export",
            "project_import",
            "project_save",
            "artifact",
            "project_save_drive",
            "editor",
            "reader",
            "search",
            "web_search",
            "drive_editor",
            "drive_update_editor",
            "drive_folder",
            "drive_reader",
            "drive_copy",
            "script_check",
            "script_run",
            "phone_file",
            "phone_folder",
            "phone_files",
            "phone_reader",
            "phone_editor",
            "phone_update_editor",
            "phone_revision_status",
        ];
        keep(
            &mut passed,
            "all27 previous Workspace operations remain reachable",
            superset(&reachable, &original_actions),
        );
        keep(
            &mut passed,
            "phone attachment is available beside phone files",
            events(&self.fire(Some(&snapshot), "phone_locations", obj(vec![])))
                .iter()
                .any(|event| event == "phone_file"),
        );
        keep(
            &mut passed,
            "document provider browsing is separate from account setup",
            same(
                &events(&self.fire(Some(&snapshot), "google_drive", obj(vec![]))),
                &["cloud_browse"],
            ),
        );
        keep(
            &mut passed,
            "app inspection remains visible under diagnostics",
            events(&self.fire(Some(&snapshot), "observability", obj(vec![])))
                .iter()
                .any(|event| event == "apps_view"),
        );

        d = self.fire(Some(state(&d)), "workspace_setup", obj(vec![]));
        let setup = events(&d);
        keep(
            &mut passed,
            "Setup exposes unique connection and permission routes",
            unique(&setup)
                && superset(
                    &setup,
                    &[
                        "phone_access",
                        "drive_edit",
                        "cloud_setup",
                        "mcp_client_settings",
                        "workspace_permissions",
                        "workspace_defaults",
                    ],
                ),
        );
        d = self.fire(Some(state(&d)), "phone_access", obj(vec![]));
        keep(
            &mut passed,
            "phone setup separates scope and permission from browsing",
            events(&d).iter().any(|event| event == "phone_folder")
                && !events(&d).iter().any(|event| event == "phone_files")
                && any_field(&d, "access", "check"),
        );
        d = self.fire(Some(state(&d)), "close", obj(vec![]));
        keep(
            &mut passed,
            "phone setup returns to Setup",
            str_at(state(&d), &["dialog"]) == Some("workspace_setup"),
        );

        d = self.fire(Some(state(&d)), "workspace", obj(vec![]));
        d = self.fire(Some(state(&d)), "workspace_projects", obj(vec![]));
        keep(
            &mut passed,
            "workspace creation and selection are visible",
            superset(
                &events(&d),
                &[
                    "workspace_new",
                    "workspace_locations",
                    "workspace_files",
                    "workspace_snapshots",
                ],
            ),
        );
        d = self.fire(Some(state(&d)), "workspace_new", obj(vec![]));
        d = self.fire(
            Some(state(&d)),
            "workspace_create",
            obj(vec![("name".into(), s("Build tools"))]),
        );
        let selected = str_at(state(&d), &["workspace_selected"])
            .unwrap_or_else(|| panic!("workspace creation did not select a workspace"))
            .to_string();
        let new_root = self.base.join(format!("workspace-{selected}"));
        keep(
            &mut passed,
            "New workspace creates its actual separate directory",
            new_root.is_dir()
                && str_at(&d, &["effect", "kind"]) == Some("workspace_selected")
                && new_root != self.original,
        );
        keep(
            &mut passed,
            "creation preserves original files",
            std::fs::read_to_string(self.original.join("keep.txt")).ok().as_deref()
                == Some("Original saved work\n"),
        );
        std::fs::write(new_root.join("new.txt"), "New workspace only\n")
            .unwrap_or_else(|error| panic!("new.txt: {error}"));
        d = self.fire(Some(state(&d)), "files", obj(vec![]));
        let effect = field(&d, "effect").clone();
        let new_root_text = new_root
            .to_str()
            .unwrap_or_else(|| panic!("new workspace path is not UTF-8"))
            .to_string();
        let listing = self.controller.op(&overlay(
            &effect,
            &[("op", s("tool")), ("root", s(&new_root_text))],
        ));
        let listing_text = listing.str_().to_string();
        keep(
            &mut passed,
            "project browsing uses the selected workspace",
            listing_text.contains("new.txt") && !listing_text.contains("keep.txt"),
        );
        let mut tool_inputs = vec![
            ("purpose".into(), s("local_tool")),
            ("output".into(), listing),
        ];
        for key in ["tool_ticket", "tool_binding", "editor", "editor_dialog"] {
            if let Some(value) = effect.get(key) {
                tool_inputs.push((key.to_string(), value.clone()));
            }
        }
        d = self.fire(Some(state(&d)), "tool_result", Value::Obj(tool_inputs));
        d = self.fire(Some(state(&d)), "workspace_select:default", obj(vec![]));
        keep(
            &mut passed,
            "original workspace selection preserves its files",
            falsy_get(state(&d), "workspace_selected")
                && self.original.join("keep.txt").exists()
                && new_root.join("new.txt").exists(),
        );
        d = self.fire(
            Some(state(&d)),
            &format!("workspace_select:{selected}"),
            obj(vec![]),
        );
        keep(
            &mut passed,
            "saved workspace can be selected again",
            str_at(state(&d), &["workspace_selected"]) == Some(selected.as_str()),
        );

        let mut busy = state(&d).clone();
        assign(&mut busy, "busy", twin::b(true));
        let blocked = self.fire(Some(&busy), "workspace_select:default", obj(vec![]));
        keep(
            &mut passed,
            "active work retains its workspace root",
            present_null(&blocked, "effect")
                && str_at(state(&blocked), &["workspace_selected"]) == Some(selected.as_str()),
        );

        d = self.permissions(state(&d), &[("phone", false), ("network", false)]);
        let mut held = state(&d).clone();
        assign(
            &mut held,
            "drive",
            obj(vec![
                ("url".into(), s(&format!("{endpoint}/files/"))),
                ("user".into(), s("kept-user")),
                ("password".into(), s("kept-password")),
                ("plain".into(), twin::b(true)),
            ]),
        );
        d = self.fire(Some(&held), "phone_files", obj(vec![]));
        keep(
            &mut passed,
            "phone access toggle blocks the actual file effect",
            present_null(&d, "effect")
                && falsy_present(state(&d), "busy")
                && str_at(state(&d), &["notice"]).is_some_and(|notice| notice.contains("access is off")),
        );
        d = self.fire(Some(state(&d)), "drive_list", obj(vec![]));
        keep(
            &mut passed,
            "network access toggle blocks the actual request",
            present_null(&d, "effect")
                && falsy_present(state(&d), "busy")
                && str_at(state(&d), &["notice"]).is_some_and(|notice| notice.contains("access is off")),
        );
        keep(
            &mut passed,
            "access toggles preserve connection credentials",
            str_at(state(&d), &["drive", "password"]) == Some("kept-password"),
        );
        d = self.fire(Some(state(&d)), "phone_folder", obj(vec![]));
        keep(
            &mut passed,
            "folder setup remains available while source access is off",
            str_at(&d, &["effect", "kind"]) == Some("document_tree"),
        );
        d = self.permissions(state(&d), &[("phone", true), ("network", true)]);

        d = self.fire(Some(state(&d)), "workspace_setup", obj(vec![]));
        d = self.fire(Some(state(&d)), "mcp_client_settings", obj(vec![]));
        keep(
            &mut passed,
            "MCP setup offers save and discovery",
            events(&d).iter().any(|event| event == "mcp_client_save_discover") && any_field(&d, "access", "check"),
        );
        keep(
            &mut passed,
            "MCP setup exposes selected-agent tool permission",
            any_field(&d, "agent_access", "check"),
        );

        let mut agents = state(&d).clone();
        assign(
            &mut agents,
            "agents",
            Value::Arr(vec![obj(vec![
                ("name".into(), s("Build agent")),
                ("instructions".into(), s("Retain this instruction")),
                ("tools".into(), s("list,read,write")),
            ])]),
        );
        d = self.fire(
            Some(&agents),
            "mcp_client_save",
            obj(vec![
                ("endpoint".into(), s(&endpoint)),
                ("key".into(), s("controlled-token")),
                ("access".into(), twin::b(true)),
                ("agent_access".into(), twin::b(true)),
            ]),
        );
        keep(
            &mut passed,
            "MCP permission adds only its selected-agent tool",
            dig(state(&d), &["agents"])
                == Some(&Value::Arr(vec![obj(vec![
                    ("name".into(), s("Build agent")),
                    ("instructions".into(), s("Retain this instruction")),
                    ("tools".into(), s("list,read,write,mcp")),
                ])])),
        );
        keep(
            &mut passed,
            "connection stores credentials separately from permission controls",
            dig(state(&d), &["mcp"])
                == Some(&obj(vec![
                    ("endpoint".into(), s(&endpoint)),
                    ("key".into(), s("controlled-token")),
                ])),
        );
        d = self.fire(Some(state(&d)), "mcp_client_settings", obj(vec![]));
        d = self.fire(
            Some(state(&d)),
            "mcp_client_save",
            obj(vec![("agent_access".into(), twin::b(false))]),
        );
        keep(
            &mut passed,
            "MCP permission can be removed while retaining setup",
            str_at(state(&d), &["agents", "0", "tools"]) == Some("list,read,write")
                && str_at(state(&d), &["mcp", "endpoint"]) == Some(endpoint.as_str()),
        );
        d = self.fire(Some(state(&d)), "mcp_client_settings", obj(vec![]));
        d = self.fire(
            Some(state(&d)),
            "mcp_client_save_discover",
            obj(vec![("agent_access".into(), twin::b(true))]),
        );
        d = self.exchange(d);
        let tool_labels = button_labels(&d);
        keep(
            &mut passed,
            "real MCP discovery opens the named tool list",
            str_at(state(&d), &["dialog"]) == Some("mcp_tools")
                && tool_labels.len() >= 2
                && tool_labels[0] == "project.create"
                && tool_labels[1] == "project.build",
        );
        keep(
            &mut passed,
            "server received initialization, notification and listing",
            first_methods(&self.peer.calls())
                == ["initialize", "notifications/initialized", "tools/list"],
        );

        d = self.fire(Some(state(&d)), "mcp_tool:0", obj(vec![]));
        d = self.fire(
            Some(state(&d)),
            "mcp_client_review",
            obj(vec![("arguments".into(), s("{\"name\":\"demo\"}"))]),
        );
        keep(
            &mut passed,
            "MCP review keeps connection credentials out of its UI",
            !dump(field(&d, "ui")).contains("controlled-token"),
        );
        let review = state(&d).clone();
        let mut changed = review.clone();
        assign_path(
            &mut changed,
            &["mcp", "endpoint"],
            s(&format!("{endpoint}/changed")),
        );
        let blocked = self.fire(Some(&changed), "mcp_client_approve", obj(vec![]));
        keep(
            &mut passed,
            "changed MCP connection retires its reviewed approval",
            present_null(&blocked, "effect")
                && falsy_present(state(&blocked), "busy")
                && str_at(state(&blocked), &["notice"])
                    .is_some_and(|notice| notice.contains("review the call again")),
        );
        d = self.fire(Some(&review), "close", obj(vec![]));
        keep(
            &mut passed,
            "Back releases the manual MCP review and retains its draft",
            absent_or_null(state(&d), "mcp_client_pending")
                && str_at(state(&d), &["dialog"]) == Some("mcp_client_call")
                && str_at(state(&d), &["edit", "arguments"]) == Some("{\"name\":\"demo\"}"),
        );
        let blocked = self.fire(Some(state(&d)), "mcp_client_approve", obj(vec![]));
        keep(
            &mut passed,
            "an approval after Back sends no request",
            present_null(&blocked, "effect") && falsy_present(state(&blocked), "busy"),
        );

        let calls = [
            (0usize, obj(vec![("name".into(), s("demo"))])),
            (
                1usize,
                obj(vec![
                    ("name".into(), s("demo")),
                    ("source".into(), s("Actual server build source\n")),
                ]),
            ),
        ];
        for (index, arguments) in calls {
            d = self.fire(Some(state(&d)), "mcp_tools", obj(vec![]));
            d = self.fire(Some(state(&d)), &format!("mcp_tool:{index}"), obj(vec![]));
            let names = ["project.create", "project.build"];
            keep(
                &mut passed,
                &format!("selected MCP tool {index} carries its exact name and schema"),
                str_at(state(&d), &["edit", "tool"]) == Some(names[index])
                    && dump(dig(state(&d), &["edit", "inputSchema"]).unwrap_or(&Value::Null)).contains("name"),
            );
            let count = tool_call_count(&self.peer.calls());
            d = self.fire(
                Some(state(&d)),
                "mcp_client_review",
                obj(vec![("arguments".into(), s(&arguments.to_string()))]),
            );
            keep(
                &mut passed,
                &format!("MCP review {index} sends no call before approval"),
                present_null(&d, "effect") && tool_call_count(&self.peer.calls()) == count,
            );
            d = self.fire(Some(state(&d)), "mcp_client_approve", obj(vec![]));
            d = self.exchange(d);
            let demo = self
                .peer
                .remote()
                .join("demo")
                .to_str()
                .unwrap_or_else(|| panic!("demo path is not UTF-8"))
                .to_string();
            keep(
                &mut passed,
                &format!("approved MCP call {index} returns the actual server output"),
                str_at(state(&d), &["dialog"]) == Some("output")
                    && falsy_present(state(&d), "busy")
                    && str_at(state(&d), &["output"]).is_some_and(|output| output.contains(&demo)),
            );
        }
        keep(
            &mut passed,
            "MCP call parameters retain only the reviewed tool and arguments",
            call_params_exact(&self.peer.calls()),
        );
        keep(
            &mut passed,
            "server build produced its actual workspace file",
            std::fs::read_to_string(self.peer.remote().join("demo/program.txt")).ok().as_deref()
                == Some("Actual server build source\n"),
        );

        let mut profiles = state(&d).clone();
        assign(&mut profiles, "profiles", Value::Arr(vec![primary_profile()]));
        d = self.fire(
            Some(&profiles),
            "send",
            obj(vec![("draft".into(), s("Discover the configured server tools for a build."))]),
        );
        keep(
            &mut passed,
            "primary sees the configured MCP command flow",
            str_at(&d, &["effect", "wire"])
                .is_some_and(|wire| wire.contains("An MCP connection is configured"))
                && str_at(&d, &["effect", "wire"]).is_some_and(|wire| !wire.contains("controlled-token")),
        );
        d = self.model_reply(&d, Some("mcp"), Some(obj(vec![("discover".into(), twin::b(true))])), "Received");
        d = self.fire(Some(state(&d)), "approve", obj(vec![]));
        d = self.exchange(d);
        keep(
            &mut passed,
            "primary receives the actual server names and schemas",
            present_value(&d, "effect")
                && str_at(&d, &["effect", "purpose"]) == Some("model")
                && str_at(&d, &["effect", "wire"]).is_some_and(|wire| wire.contains("project.build")),
        );
        d = self.model_reply(&d, None, None, "Server tools received.");

        let connections = self.fire(Some(state(&d)), "mcp_client_connections", obj(vec![]));
        keep(
            &mut passed,
            "existing MCP connection appears as the active server",
            dump(dig(&connections, &["ui", "modal"]).unwrap_or(&Value::Null)).contains(&endpoint)
                && events(&connections).iter().any(|event| event == "mcp_client_new"),
        );
        let menu = self.fire(Some(state(&d)), "chat_actions", obj(vec![]));
        keep(
            &mut passed,
            "composer plus menu starts with New conversation",
            button_rows(&menu)
                .first()
                .and_then(|button| str_at(button, &["event"]))
                == Some("new"),
        );

        let second = Peer::start(PathBuf::new(), "/second", "second-token", None, false);
        let second_endpoint = format!("{}/second", second.origin());
        let adding = self.fire(Some(state(&connections)), "mcp_client_new", obj(vec![]));
        keep(
            &mut passed,
            "Add MCP connection starts without previous credentials",
            falsy_present(state(&adding), "edit"),
        );
        let invalid = self.fire(
            Some(state(&adding)),
            "mcp_client_save",
            obj(vec![
                ("endpoint".into(), s("invalid URL")),
                ("key".into(), s("unused")),
            ]),
        );
        keep(
            &mut passed,
            "invalid new connection preserves the current connection",
            present_null(&invalid, "effect")
                && str_at(state(&invalid), &["mcp", "endpoint"]) == Some(endpoint.as_str()),
        );
        let saved = self.fire(
            Some(state(&adding)),
            "mcp_client_save_discover",
            obj(vec![
                ("name".into(), s("Second")),
                ("endpoint".into(), s(&second_endpoint)),
                ("key".into(), s("second-token")),
            ]),
        );
        keep(
            &mut passed,
            "adding a server retains the existing endpoint and token",
            dig(state(&saved), &["mcp_saved_connections"])
                == Some(&Value::Arr(vec![obj(vec![
                    ("endpoint".into(), s(&endpoint)),
                    ("key".into(), s("controlled-token")),
                ])])),
        );
        let listed = self.exchange(saved);
        keep(
            &mut passed,
            "discovery contacts the second server with its own key",
            names(dig(state(&listed), &["mcp_client_catalog"]).unwrap_or(&Value::Arr(vec![])))
                == ["second.lookup"],
        );
        let mut resumed = self.fire(Some(state(&listed)), "resume", obj(vec![]));
        resumed = self.fire(Some(state(&resumed)), "mcp_client_connections", obj(vec![]));
        let modal = dump(dig(&resumed, &["ui", "modal"]).unwrap_or(&Value::Null));
        keep(
            &mut passed,
            "both MCP servers survive state restoration",
            modal.contains("Second") && modal.contains(&endpoint),
        );
        keep(
            &mut passed,
            "connection list does not show either bearer token",
            !modal.contains("controlled-token") && !modal.contains("second-token"),
        );
        let selected_server = self.fire(Some(state(&resumed)), "mcp_client_select:0", obj(vec![]));
        keep(
            &mut passed,
            "switching retires the old server tools and keeps both credentials",
            falsy_present(state(&selected_server), "mcp_client_catalog")
                && str_at(state(&selected_server), &["mcp", "key"]) == Some("controlled-token")
                && str_at(state(&selected_server), &["mcp_saved_connections", "0", "key"])
                    == Some("second-token"),
        );
        let list_request = self.fire(
            Some(state(&selected_server)),
            "mcp_client_list",
            obj(vec![]),
        );
        let listed = self.exchange(list_request);
        let tools = str_at(state(&listed), &["mcp_client_tools"]).unwrap_or("");
        keep(
            &mut passed,
            "switching back discovers the original tools again",
            tools.contains("project.build") && !tools.contains("second.lookup"),
        );
        d = listed;
        drop(second);

        d = self.fire(Some(state(&d)), "mcp_client_list", obj(vec![]));
        let old = field(&d, "effect").clone();
        d = self.permissions(state(&d), &[("mcp", false)]);
        keep(
            &mut passed,
            "MCP access can be revoked during an active request",
            str_at(&d, &["effect", "kind"]) == Some("storage_policy")
                && falsy_path(state(&d), &["storage_access", "mcp"]),
        );
        let withheld = obj(vec![
            ("jsonrpc".into(), s("2.0")),
            (
                "id".into(),
                dig(state(&d), &["mcp_client_request_id"])
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            (
                "result".into(),
                obj(vec![
                    ("protocolVersion".into(), s("2025-11-25")),
                    ("capabilities".into(), obj(vec![("tools".into(), obj(vec![]))])),
                    (
                        "serverInfo".into(),
                        obj(vec![
                            ("name".into(), s("WITHHELD_SERVER")),
                            ("version".into(), s("1")),
                        ]),
                    ),
                ]),
            ),
        ])
        .to_string();
        let wire = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{withheld}",
            withheld.len()
        );
        d = self.fire(
            Some(state(&d)),
            "network_result",
            obj(vec![
                ("purpose".into(), field(&old, "purpose").clone()),
                ("data".into(), s(&b64_encode(wire.as_bytes()))),
            ]),
        );
        keep(
            &mut passed,
            "revoked MCP response is withheld and settles the request",
            falsy_present(state(&d), "busy")
                && !dump(field(&d, "ui")).contains("WITHHELD_SERVER")
                && present_null(&d, "effect"),
        );
        d = self.fire(Some(state(&d)), "mcp_client_list", obj(vec![]));
        keep(
            &mut passed,
            "disabled MCP access sends no new request",
            present_null(&d, "effect") && falsy_present(state(&d), "busy"),
        );
        d = self.permissions(state(&d), &[("mcp", true)]);

        d = self.fire(Some(state(&d)), "google_drive", obj(vec![]));
        d = self.fire(Some(state(&d)), "cloud_setup", obj(vec![]));
        d = self.fire(Some(state(&d)), "cloud_sign_in", obj(vec![]));
        keep(
            &mut passed,
            "document provider sign-in uses the Android account action",
            str_at(&d, &["effect", "kind"]) == Some("cloud_account"),
        );
        d = self.fire(Some(state(&d)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&d, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        d = self.fire(
            Some(state(&d)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                ("uri".into(), s("content://controlled-drive/document/source")),
                ("name".into(), s("source.txt")),
                ("mime".into(), s("text/plain")),
            ]),
        );
        keep(
            &mut passed,
            "owned document selection opens local file controls",
            str_at(state(&d), &["dialog"]) == Some("cloud_selected")
                && superset(&events(&d), &["cloud_read", "cloud_open"]),
        );
        d = self.fire(Some(state(&d)), "cloud_read", obj(vec![]));
        keep(
            &mut passed,
            "text read has an owned platform effect and active state",
            str_at(&d, &["effect", "kind"]) == Some("cloud_read") && truthy_present(state(&d), "busy"),
        );
        let ticket = str_at(&d, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document read did not return a ticket"))
            .to_string();
        d = self.fire(
            Some(state(&d)),
            "cloud_text_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                ("content".into(), s("Selected source text\n")),
            ]),
        );
        keep(
            &mut passed,
            "owned text result remains in its local source view",
            falsy_present(state(&d), "busy")
                && str_at(state(&d), &["cloud_document", "content"]) == Some("Selected source text\n"),
        );
        d = self.fire(Some(state(&d)), "cloud_copy", obj(vec![]));
        keep(
            &mut passed,
            "copying selected text uses the existing reviewed editor",
            str_at(state(&d), &["dialog"]) == Some("editor")
                && str_at(state(&d), &["edit", "content"]) == Some("Selected source text\n"),
        );
        d = self.permissions(state(&d), &[("cloud", false)]);
        d = self.fire(Some(state(&d)), "cloud_open", obj(vec![]));
        keep(
            &mut passed,
            "document provider access off blocks file opening",
            present_null(&d, "effect")
                && str_at(state(&d), &["notice"]).is_some_and(|notice| notice.contains("access is off")),
        );
        d = self.fire(Some(state(&d)), "cloud_browse", obj(vec![]));
        keep(
            &mut passed,
            "disabled document provider routes browsing to access setup",
            present_null(&d, "effect") && str_at(state(&d), &["dialog"]) == Some("workspace_permissions"),
        );
        d = self.permissions(state(&d), &[("cloud", true)]);
        d = self.fire(Some(state(&d)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&d, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        d = self.fire(Some(state(&d)), "workspace_select:default", obj(vec![]));
        d = self.fire(
            Some(state(&d)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                ("uri".into(), s("content://controlled-drive/document/stale")),
                ("name".into(), s("STALE_SOURCE")),
                ("mime".into(), s("text/plain")),
            ]),
        );
        keep(
            &mut passed,
            "workspace changes retire the old document selection",
            !dump(field(&d, "ui")).contains("STALE_SOURCE"),
        );

        let mut c = self.fire(None, "resume", obj(vec![]));
        let mut fresh = state(&c).clone();
        assign(&mut fresh, "profiles", Value::Arr(vec![primary_profile()]));
        c = self.fire(Some(&fresh), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(
            Some(state(&c)),
            "send",
            obj(vec![(
                "draft".into(),
                s("Continue this model request while the selected file metadata arrives."),
            )]),
        );
        let model_owner = dig(state(&c), &["model_expected"])
            .cloned()
            .unwrap_or_else(|| panic!("missing model owner"));
        let task_owner = py_optional(state(&c), "task_run");
        c = self.fire(
            Some(state(&c)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                (
                    "cloud_response".into(),
                    obj(vec![
                        ("uri".into(), s("content://controlled-drive/document/active")),
                        ("name".into(), s("active.txt")),
                        ("mime".into(), s("text/plain")),
                    ]),
                ),
            ]),
        );
        keep(
            &mut passed,
            "owned file selection completes while the primary remains active",
            truthy_present(state(&c), "busy")
                && dig(state(&c), &["model_expected"]) == Some(&model_owner)
                && py_optional(state(&c), "task_run") == task_owner
                && str_at(state(&c), &["cloud_document", "name"]) == Some("active.txt"),
        );
        c = self.model_reply(&c, None, None, "Primary completed after file selection.");
        keep(
            &mut passed,
            "primary response remains usable after independent file selection",
            falsy_present(state(&c), "busy"),
        );

        c = self.fire(Some(state(&c)), "cloud_read", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document read did not return a ticket"))
            .to_string();
        c = self.fire(Some(state(&c)), "cancel", obj(vec![]));
        keep(
            &mut passed,
            "cancel retires the current document provider read owner",
            absent_or_null(state(&c), "cloud_expected"),
        );
        c = self.fire(
            Some(state(&c)),
            "send",
            obj(vec![("draft".into(), s("Fresh request after the cancelled read."))]),
        );
        let snapshot = state(&c).clone();
        let stale = [
            (
                "cloud_text_result",
                obj(vec![("content".into(), s("CANCELLED_OLD_READ"))]),
            ),
            (
                "cloud_failure",
                obj(vec![("error".into(), s("CANCELLED_OLD_FAILURE"))]),
            ),
            ("cloud_cancel", obj(vec![])),
            (
                "cloud_result",
                obj(vec![
                    ("uri".into(), s("content://controlled-drive/document/old")),
                    ("name".into(), s("CANCELLED_OLD_SELECTION")),
                    ("mime".into(), s("text/plain")),
                ]),
            ),
        ];
        for (action, payload) in stale {
            c = self.fire(
                Some(&snapshot),
                action,
                obj(vec![
                    ("ticket".into(), s(&ticket)),
                    ("cloud_response".into(), payload),
                ]),
            );
            keep(
                &mut passed,
                &format!("{action} from a cancelled read leaves the newer model and UI unchanged"),
                state(&c) == &snapshot && present_null(&c, "effect"),
            );
        }
        c = self.model_reply(&c, None, None, "Fresh request completed.");
        keep(
            &mut passed,
            "new model callback still completes after all stale file callbacks",
            falsy_present(state(&c), "busy"),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(Some(state(&c)), "resume", obj(vec![]));
        let snapshot = state(&c).clone();
        keep(
            &mut passed,
            "application recovery retires its previous document request",
            absent_or_null(&snapshot, "cloud_expected"),
        );
        c = self.fire(
            Some(&snapshot),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                (
                    "cloud_response".into(),
                    obj(vec![
                        ("uri".into(), s("content://controlled-drive/document/recovered")),
                        ("name".into(), s("RECOVERED_OLD_FILE")),
                        ("mime".into(), s("text/plain")),
                    ]),
                ),
            ]),
        );
        keep(
            &mut passed,
            "recovered old selection is an exact state-preserving no-op",
            state(&c) == &snapshot && present_null(&c, "effect"),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(Some(state(&c)), "agent_new", obj(vec![]));
        c = self.fire(
            Some(state(&c)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                ("name".into(), s("Typed agent name")),
                ("instructions".into(), s("Typed agent instructions")),
                (
                    "cloud_response".into(),
                    obj(vec![
                        ("uri".into(), s("content://controlled-drive/document/named")),
                        ("name".into(), s("drive-file.txt")),
                        ("mime".into(), s("text/plain")),
                    ]),
                ),
            ]),
        );
        keep(
            &mut passed,
            "nested document metadata preserves the actual typed agent name",
            str_at(state(&c), &["edit", "name"]) == Some("Typed agent name")
                && str_at(state(&c), &["edit", "instructions"]) == Some("Typed agent instructions"),
        );
        keep(
            &mut passed,
            "document result is retained separately from the settings form",
            str_at(state(&c), &["cloud_document", "name"]) == Some("drive-file.txt")
                && ["uri", "mime", "ticket", "cloud_response"]
                    .iter()
                    .all(|key| dig(state(&c), &["edit"]).and_then(|edit| edit.get(key)).is_none()),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(Some(state(&c)), "agent_new", obj(vec![]));
        c = self.fire(
            Some(state(&c)),
            "capture",
            obj(vec![("name".into(), s("Saved draft name"))]),
        );
        c = self.fire(
            Some(state(&c)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                ("uri".into(), s("content://controlled-drive/document/legacy")),
                ("name".into(), s("legacy-file.txt")),
                ("mime".into(), s("text/plain")),
            ]),
        );
        keep(
            &mut passed,
            "legacy flat metadata retains the previously captured form name",
            str_at(state(&c), &["edit", "name"]) == Some("Saved draft name")
                && str_at(state(&c), &["cloud_document", "name"]) == Some("legacy-file.txt"),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        c = self.fire(Some(state(&c)), "agent_new", obj(vec![]));
        let snapshot = state(&c).clone();
        c = self.fire(
            Some(&snapshot),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s("stale-ticket")),
                ("name".into(), s("STALE_FORM_OVERWRITE")),
                (
                    "cloud_response".into(),
                    obj(vec![
                        ("uri".into(), s("content://controlled-drive/document/stale")),
                        ("name".into(), s("STALE_DOCUMENT")),
                        ("mime".into(), s("text/plain")),
                    ]),
                ),
            ]),
        );
        keep(
            &mut passed,
            "unowned file callback is rejected before any form capture",
            state(&c) == &snapshot && present_null(&c, "effect"),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(
            Some(state(&c)),
            "send",
            obj(vec![("draft".into(), s("Keep working while the picker is cancelled."))]),
        );
        let model_owner = dig(state(&c), &["model_expected"])
            .cloned()
            .unwrap_or_else(|| panic!("missing model owner"));
        c = self.fire(
            Some(state(&c)),
            "cloud_cancel",
            obj(vec![("ticket".into(), s(&ticket))]),
        );
        keep(
            &mut passed,
            "picker cancellation settles only its selection owner",
            truthy_present(state(&c), "busy")
                && dig(state(&c), &["model_expected"]) == Some(&model_owner)
                && absent_or_null(state(&c), "cloud_expected"),
        );
        c = self.model_reply(&c, None, None, "Primary completed after picker cancellation.");
        keep(
            &mut passed,
            "primary completes normally after picker cancellation",
            falsy_present(state(&c), "busy"),
        );

        c = self.fire(Some(state(&c)), "cloud_browse", obj(vec![]));
        let ticket = str_at(&c, &["effect", "ticket"])
            .unwrap_or_else(|| panic!("document browse did not return a ticket"))
            .to_string();
        c = self.fire(
            Some(state(&c)),
            "send",
            obj(vec![("draft".into(), s("Keep working while storage access changes."))]),
        );
        let model_owner = dig(state(&c), &["model_expected"])
            .cloned()
            .unwrap_or_else(|| panic!("missing model owner"));
        c = self.permissions(state(&c), &[("cloud", false)]);
        c = self.fire(
            Some(state(&c)),
            "cloud_result",
            obj(vec![
                ("ticket".into(), s(&ticket)),
                (
                    "cloud_response".into(),
                    obj(vec![
                        ("uri".into(), s("content://controlled-drive/document/revoked")),
                        ("name".into(), s("WITHHELD_CLOUD_SELECTION")),
                        ("mime".into(), s("text/plain")),
                    ]),
                ),
            ]),
        );
        keep(
            &mut passed,
            "revoked selection withholds data without settling unrelated model work",
            truthy_present(state(&c), "busy")
                && dig(state(&c), &["model_expected"]) == Some(&model_owner)
                && !dump(field(&c, "ui")).contains("WITHHELD_CLOUD_SELECTION"),
        );
        c = self.model_reply(&c, None, None, "Primary completed with cloud access off.");
        keep(
            &mut passed,
            "source revocation preserves the ordinary primary completion",
            falsy_present(state(&c), "busy"),
        );
        passed
    }

    fn fire(&mut self, st: Option<&Value>, action: &str, inputs: Value) -> Value {
        let root = workspace_root(&self.base, &self.original, st);
        self.controller.event(st, action, inputs, &root)
    }

    fn permissions(&mut self, st: &Value, changes: &[(&str, bool)]) -> Value {
        let opened = self.fire(Some(st), "workspace_permissions", obj(vec![]));
        let mut values = vec![
            ("phone".to_string(), twin::b(true)),
            ("network".to_string(), twin::b(true)),
            ("cloud".to_string(), twin::b(true)),
            ("mcp".to_string(), twin::b(true)),
        ];
        match state(&opened).get("storage_access") {
            None => {}
            Some(Value::Obj(fields)) => {
                for (key, value) in fields {
                    upsert(&mut values, key, value.clone());
                }
            }
            Some(other) => panic!("storage_access is {}", other.to_string()),
        }
        for (key, value) in changes {
            upsert(&mut values, key, twin::b(*value));
        }
        self.fire(Some(state(&opened)), "workspace_access_save", Value::Obj(values))
    }

    fn exchange(&mut self, mut current: Value) -> Value {
        for _ in 0..8 {
            let Some(effect) = current.get("effect").cloned() else {
                return current;
            };
            if !twin::truthy(&effect) {
                return current;
            }
            match effect.get("kind") {
                Some(Value::Str(kind)) if kind == "network" => {}
                Some(Value::Str(_)) => return current,
                _ => panic!("network effect is missing kind"),
            }
            if matches!(effect.get("purpose"), Some(Value::Str(purpose)) if purpose == "model") {
                return current;
            }
            let raw = http_exchange(&effect);
            let held = state(&current).clone();
            current = self.fire(
                Some(&held),
                "network_result",
                obj(vec![
                    ("purpose".into(), field(&effect, "purpose").clone()),
                    ("data".into(), s(&b64_encode(&raw))),
                ]),
            );
        }
        panic!("MCP exchange exceeded the bounded sequence");
    }

    fn model_reply(&mut self, current: &Value, tool: Option<&str>, args: Option<Value>, text: &str) -> Value {
        let content = if let Some(tool) = tool {
            obj(vec![
                ("tool".into(), s(tool)),
                ("args".into(), args.unwrap_or_else(|| obj(vec![]))),
            ])
            .to_string()
        } else {
            text.to_string()
        };
        let body = obj(vec![(
            "choices".into(),
            Value::Arr(vec![obj(vec![
                (
                    "message".into(),
                    obj(vec![("content".into(), s(&content))]),
                ),
                ("finish_reason".into(), s("stop")),
            ])]),
        )])
        .to_string();
        let wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let held = state(current).clone();
        self.fire(
            Some(&held),
            "network_result",
            obj(vec![
                ("purpose".into(), s("model")),
                (
                    "model_ticket".into(),
                    dig(&held, &["model_expected"]).cloned().unwrap_or(Value::Null),
                ),
                ("data".into(), s(&b64_encode(wire.as_bytes()))),
            ]),
        )
    }
}

fn primary_profile() -> Value {
    obj(vec![
        ("name".into(), s("Controlled primary")),
        ("model".into(), s("primary")),
        ("endpoint".into(), s("http://primary.test/model")),
        ("plain".into(), twin::b(true)),
        ("stream".into(), twin::b(false)),
        ("protocol".into(), s("chat")),
    ])
}

fn workspace_root(base: &std::path::Path, original: &std::path::Path, st: Option<&Value>) -> String {
    match st.and_then(|state| state.get("workspace_selected")) {
        Some(value) if twin::truthy(value) => base
            .join(format!("workspace-{}", value.str_()))
            .to_str()
            .unwrap_or_else(|| panic!("workspace path is not UTF-8"))
            .to_string(),
        _ => original
            .to_str()
            .unwrap_or_else(|| panic!("workspace path is not UTF-8"))
            .to_string(),
    }
}

fn http_exchange(effect: &Value) -> Vec<u8> {
    let host = field(effect, "host").str_();
    let port = match field(effect, "port") {
        Value::Num(text) | Value::Str(text) => text
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("bad port {text}: {error}")),
        other => panic!("bad port {}", other.to_string()),
    };
    let wire = field(effect, "wire").str_();
    let mut stream = TcpStream::connect((host, port)).unwrap_or_else(|error| panic!("connect {host}:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("read timeout: {error}"));
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|error| panic!("write timeout: {error}"));
    stream
        .write_all(wire.as_bytes())
        .unwrap_or_else(|error| panic!("write: {error}"));
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("read: {error}"));
    response
}

fn keep(passed: &mut Vec<String>, name: &str, ok: bool) {
    if !ok {
        panic!("{name}");
    }
    passed.push(name.to_string());
}

fn state(value: &Value) -> &Value {
    field(value, "state")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("missing {key} in {}", value.to_string()))
}

fn dig<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for (depth, key) in path.iter().enumerate() {
        current = match current {
            Value::Arr(items) => {
                let index = key.parse::<usize>().ok()?;
                items.get(index)?
            }
            Value::Obj(_) => current.get(key)?,
            _ => return None,
        };
        if depth + 1 == path.len() {
            return Some(current);
        }
    }
    Some(current)
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    match dig(value, path)? {
        Value::Str(text) => Some(text),
        _ => None,
    }
}

fn falsy_get(value: &Value, key: &str) -> bool {
    !value.get(key).is_some_and(twin::truthy)
}

fn falsy_present(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(item) => !twin::truthy(item),
        None => false,
    }
}

fn falsy_path(value: &Value, path: &[&str]) -> bool {
    match dig(value, path) {
        Some(item) => !twin::truthy(item),
        None => false,
    }
}

fn truthy_present(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(twin::truthy)
}

fn present_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Null))
}

fn present_value(value: &Value, key: &str) -> bool {
    value.get(key).is_some_and(|item| !matches!(item, Value::Null))
}

fn absent_or_null(value: &Value, key: &str) -> bool {
    matches!(value.get(key), None | Some(Value::Null))
}

fn py_optional(value: &Value, key: &str) -> Option<Value> {
    match value.get(key) {
        None | Some(Value::Null) => None,
        Some(other) => Some(other.clone()),
    }
}

fn dump(value: &Value) -> String {
    value.to_string()
}

fn button_rows(value: &Value) -> Vec<Value> {
    let Some(ui) = value.get("ui") else {
        return Vec::new();
    };
    if let Some(modal) = ui.get("modal") {
        if let Some(Value::Arr(buttons)) = modal.get("buttons") {
            return buttons.clone();
        }
    }
    match ui.get("rows") {
        Some(Value::Arr(rows)) => rows.clone(),
        _ => Vec::new(),
    }
}

fn events(value: &Value) -> Vec<String> {
    button_rows(value)
        .iter()
        .filter_map(|button| str_at(button, &["event"]).map(str::to_string))
        .collect()
}

fn button_labels(value: &Value) -> Vec<String> {
    button_rows(value)
        .iter()
        .filter_map(|button| str_at(button, &["label"]).map(str::to_string))
        .collect()
}

fn labels(value: &Value) -> Vec<String> {
    match dig(value, &["ui", "rows"]) {
        Some(Value::Arr(rows)) => rows
            .iter()
            .filter_map(|row| str_at(row, &["label"]).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn same(have: &[String], need: &[&str]) -> bool {
    have.len() == need.len() && have.iter().zip(need).all(|(have, need)| have == need)
}

fn superset(have: &[String], need: &[&str]) -> bool {
    need.iter().all(|item| have.iter().any(|value| value == item))
}

fn unique(items: &[String]) -> bool {
    items.iter().enumerate().all(|(index, item)| !items[..index].iter().any(|earlier| earlier == item))
}

fn any_field(value: &Value, key: &str, kind: &str) -> bool {
    match dig(value, &["ui", "modal", "fields"]) {
        Some(Value::Arr(fields)) => fields.iter().any(|field| {
            str_at(field, &["key"]) == Some(key) && str_at(field, &["kind"]) == Some(kind)
        }),
        _ => false,
    }
}

fn names(value: &Value) -> Vec<String> {
    match value {
        Value::Arr(items) => items
            .iter()
            .filter_map(|item| str_at(item, &["name"]).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn assign(value: &mut Value, key: &str, next: Value) {
    let Value::Obj(fields) = value else {
        panic!("expected object, got {}", value.to_string());
    };
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = next;
    } else {
        fields.push((key.to_string(), next));
    }
}

fn assign_path(value: &mut Value, path: &[&str], next: Value) {
    let mut current = value;
    for key in &path[..path.len() - 1] {
        let next_current = match current {
            Value::Arr(items) => {
                let index = key.parse::<usize>().unwrap_or_else(|_| panic!("expected index, got {key}"));
                items.get_mut(index).unwrap_or_else(|| panic!("missing index {index}"))
            }
            Value::Obj(fields) => {
                let index = fields.iter().position(|(name, _)| name == key).unwrap_or_else(|| panic!("missing {key}"));
                &mut fields[index].1
            }
            other => panic!("expected object or array, got {}", other.to_string()),
        };
        current = next_current;
    }
    assign(current, path[path.len() - 1], next);
}

fn upsert(fields: &mut Vec<(String, Value)>, key: &str, value: Value) {
    if let Some(index) = fields.iter().position(|(name, _)| name == key) {
        fields[index].1 = value;
    } else {
        fields.push((key.to_string(), value));
    }
}

fn overlay(effect: &Value, pairs: &[(&str, Value)]) -> Value {
    let Value::Obj(fields) = effect else {
        panic!("effect is not an object: {}", effect.to_string());
    };
    let mut fields = fields.clone();
    for (key, value) in pairs {
        if let Some(index) = fields.iter().position(|(name, _)| name == key) {
            fields[index].1 = value.clone();
        } else {
            fields.push(((*key).to_string(), value.clone()));
        }
    }
    Value::Obj(fields)
}

#[derive(Clone)]
struct RpcCall {
    method: String,
    params: Value,
}

fn first_methods(calls: &[RpcCall]) -> [&str; 3] {
    if calls.len() < 3 {
        return ["", "", ""];
    }
    [
        calls[0].method.as_str(),
        calls[1].method.as_str(),
        calls[2].method.as_str(),
    ]
}

fn tool_call_count(calls: &[RpcCall]) -> usize {
    calls.iter().filter(|call| call.method == "tools/call").count()
}

fn call_params_exact(calls: &[RpcCall]) -> bool {
    calls.iter().filter(|call| call.method == "tools/call").all(|call| match &call.params {
        Value::Obj(fields) => {
            fields.len() == 2
                && fields.iter().any(|(key, _)| key == "name")
                && fields.iter().any(|(key, _)| key == "arguments")
        }
        _ => false,
    })
}

struct Peer {
    origin: String,
    port: u16,
    remote: PathBuf,
    state: Arc<Mutex<PeerState>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct PeerState {
    calls: Vec<RpcCall>,
    remote: PathBuf,
    path: String,
    token: String,
    session: Option<String>,
    primary: bool,
}

impl Peer {
    fn start(remote: PathBuf, path: &str, token: &str, session: Option<&str>, primary: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("peer bind: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("peer address: {error}"))
            .port();
        let origin = format!("http://127.0.0.1:{port}");
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(PeerState {
            calls: Vec::new(),
            remote: remote.clone(),
            path: path.to_string(),
            token: token.to_string(),
            session: session.map(str::to_string),
            primary,
        }));
        let flag = Arc::clone(&shutdown);
        let thread_state = Arc::clone(&state);
        let thread = thread::spawn(move || {
            loop {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if flag.load(Ordering::SeqCst) {
                            break;
                        }
                        let state = Arc::clone(&thread_state);
                        thread::spawn(move || {
                            if let Err(error) = serve(stream, &state) {
                                panic!("mcp peer: {error}");
                            }
                        });
                    }
                    Err(error) => {
                        if flag.load(Ordering::SeqCst) {
                            break;
                        }
                        panic!("accept: {error}");
                    }
                }
            }
        });
        Self {
            origin,
            port,
            remote,
            state,
            shutdown,
            thread: Some(thread),
        }
    }

    fn origin(&self) -> &str {
        &self.origin
    }

    fn remote(&self) -> &PathBuf {
        &self.remote
    }

    fn calls(&self) -> Vec<RpcCall> {
        self.state
            .lock()
            .unwrap_or_else(|error| panic!("peer lock: {error}"))
            .calls
            .clone()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: TcpStream, state: &Arc<Mutex<PeerState>>) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let request = read_request(&mut stream)?;
    let peer = state.lock().map_err(|_| "peer lock".to_string())?;
    if request.path != peer.path || request.authorization.as_deref() != Some(format!("Bearer {}", peer.token)).as_deref() {
        return Err(format!(
            "unexpected MCP request {} {:?}",
            request.path, request.authorization
        ));
    }
    let message = Value::parse(&request.body);
    let method = message
        .get("method")
        .and_then(|value| match value {
            Value::Str(text) => Some(text.clone()),
            _ => None,
        })
        .ok_or("missing MCP method")?;
    let params = message.get("params").cloned().unwrap_or_else(|| Value::Obj(vec![]));
    if peer.primary {
        // Record after the request is authenticated, before the response, matching the leg.
    }
    let primary = peer.primary;
    let session = peer.session.clone();
    let remote = peer.remote.clone();
    if primary {
        let mut recorded = peer;
        recorded.calls.push(RpcCall {
            method: method.clone(),
            params: params.clone(),
        });
        drop(recorded);
    } else {
        drop(peer);
    }
    if method == "notifications/initialized" {
        return write_response(&mut stream, 202, "Accepted", &[], b"");
    }
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let result = if method == "initialize" {
        obj(vec![
            ("protocolVersion".into(), s("2025-11-25")),
            ("capabilities".into(), obj(vec![("tools".into(), obj(vec![]))])),
            (
                "serverInfo".into(),
                obj(vec![
                    (
                        "name".into(),
                        s(if primary { "Controlled network build server" } else { "Second server" }),
                    ),
                    ("version".into(), s("1")),
                ]),
            ),
        ])
    } else if method == "tools/list" || !primary {
        if primary {
            obj(vec![(
                "tools".into(),
                Value::Arr(vec![
                    tool(
                        "project.create",
                        "Create a server workspace.",
                        obj(vec![
                            ("type".into(), s("object")),
                            (
                                "properties".into(),
                                obj(vec![("name".into(), obj(vec![("type".into(), s("string"))]))]),
                            ),
                            ("required".into(), Value::Arr(vec![s("name")])),
                        ]),
                    ),
                    tool(
                        "project.build",
                        "Write the selected source into the server workspace.",
                        obj(vec![
                            ("type".into(), s("object")),
                            (
                                "properties".into(),
                                obj(vec![
                                    ("name".into(), obj(vec![("type".into(), s("string"))])),
                                    ("source".into(), obj(vec![("type".into(), s("string"))])),
                                ]),
                            ),
                            ("required".into(), Value::Arr(vec![s("name"), s("source")])),
                        ]),
                    ),
                ]),
            )])
        } else {
            obj(vec![(
                "tools".into(),
                Value::Arr(vec![obj(vec![
                    ("name".into(), s("second.lookup")),
                    ("inputSchema".into(), obj(vec![("type".into(), s("object"))])),
                ])]),
            )])
        }
    } else {
        if !exact_object(&params, &["name", "arguments"]) {
            return Err("Approval ownership left the app".into());
        }
        let arguments = field(&params, "arguments");
        let name = field(arguments, "name").str_();
        if name != "demo" {
            return Err(format!("unexpected workspace {name}"));
        }
        let folder = remote.join(name);
        let artifact = if field(&params, "name").str_() == "project.create" {
            std::fs::create_dir_all(&folder).map_err(|error| format!("create workspace: {error}"))?;
            obj(vec![(
                "workspace".into(),
                s(folder.to_str().ok_or("workspace path is not UTF-8")?),
            )])
        } else {
            let source = field(arguments, "source").str_();
            let output = folder.join("program.txt");
            std::fs::write(&output, source.as_bytes()).map_err(|error| format!("write source: {error}"))?;
            let bytes = std::fs::read(&output).map_err(|error| format!("read source: {error}"))?;
            obj(vec![
                (
                    "artifact".into(),
                    s(output.to_str().ok_or("artifact path is not UTF-8")?),
                ),
                ("sha256".into(), s(&twin::sha256_bytes(&bytes))),
            ])
        };
        obj(vec![(
            "content".into(),
            Value::Arr(vec![obj(vec![
                ("type".into(), s("text")),
                ("text".into(), s(&artifact.to_string())),
            ])]),
        )])
    };
    let body = obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), id),
        ("result".into(), result),
    ])
    .to_string();
    let mut headers = Vec::new();
    if primary {
        headers.push(("Content-Type", "application/json"));
        if let Some(session) = session.as_deref() {
            headers.push(("Mcp-Session-Id", session));
        }
    }
    write_response(&mut stream, 200, "OK", &headers, body.as_bytes())
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    obj(vec![
        ("name".into(), s(name)),
        ("description".into(), s(description)),
        ("inputSchema".into(), schema),
    ])
}

fn exact_object(value: &Value, keys: &[&str]) -> bool {
    let Value::Obj(fields) = value else {
        return false;
    };
    fields.len() == keys.len() && keys.iter().all(|key| fields.iter().any(|(name, _)| name == key))
}

struct HttpRequest {
    path: String,
    authorization: Option<String>,
    body: String,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("HTTP request exceeds 8 MiB".into());
        }
        let count = stream.read(&mut buffer).map_err(|error| format!("read request: {error}"))?;
        if count == 0 {
            return Err("incomplete HTTP request".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "request headers are not UTF-8".to_string())?;
        let mut lines = header.split("\r\n");
        let request_line = lines.next().ok_or("missing request line")?;
        let path = request_line.split_whitespace().nth(1).ok_or("missing request path")?.to_string();
        let mut length = None;
        let mut authorization = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                return Err(format!("bad header {line}"));
            };
            if name.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().map_err(|_| "bad content-length".to_string())?);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            }
        }
        let length = length.ok_or("missing content-length")?;
        let start = header_end + 4;
        if bytes.len() < start + length {
            continue;
        }
        if bytes.len() != start + length {
            return Err("trailing bytes after HTTP request".into());
        }
        let body = std::str::from_utf8(&bytes[start..]).map_err(|_| "request body is not UTF-8".to_string())?.to_string();
        return Ok(HttpRequest { path, authorization, body });
    }
}

fn write_response(stream: &mut TcpStream, status: u16, reason: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<(), String> {
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("write response: {error}"))?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}
