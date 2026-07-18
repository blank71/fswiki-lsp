use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl LspProcess {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fswiki-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start fswiki-lsp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_vec(message).expect("serialize message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        self.stdin.write_all(&body).expect("write body");
        self.stdin.flush().expect("flush message");
    }

    fn receive_message(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut header = String::new();
            self.stdout.read_line(&mut header).expect("read header");
            assert!(!header.is_empty(), "server closed stdout");
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                content_length = Some(value.trim().parse::<usize>().expect("content length"));
            }
        }

        let mut body = vec![0; content_length.expect("Content-Length header")];
        self.stdout.read_exact(&mut body).expect("read body");
        serde_json::from_slice(&body).expect("parse response")
    }

    fn receive(&mut self) -> Value {
        loop {
            let message = self.receive_message();
            if message.get("id").is_some() {
                return message;
            }
        }
    }

    fn receive_notification(&mut self, method: &str) -> Value {
        self.receive_notification_matching(method, |_| true)
    }

    fn receive_notification_matching(
        &mut self,
        method: &str,
        predicate: impl Fn(&Value) -> bool,
    ) -> Value {
        loop {
            let message = self.receive_message();
            if message["method"] == method && predicate(&message) {
                return message;
            }
        }
    }

    fn stop(mut self) {
        self.send(&json!({"jsonrpc": "2.0", "id": 99, "method": "shutdown"}));
        assert_eq!(self.receive()["id"], 99);
        self.send(&json!({"jsonrpc": "2.0", "method": "exit"}));
        assert!(self.child.wait().expect("wait for server").success());
    }
}

fn assert_plugin_completion(lsp: &mut LspProcess) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 2},
            "contentChanges": [{"text": "{"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "textDocument/completion",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "position": {"line": 0, "character": 1},
            "context": {"triggerKind": 2, "triggerCharacter": "{"}
        }
    }));
    let completions = lsp.receive();
    assert_eq!(completions["id"], 5);
    assert_eq!(completions["result"][0]["label"], "pre");
    assert_eq!(
        completions["result"][2]["textEdit"]["newText"],
        "{{ref ${1:some.png}}}$0"
    );
    assert_eq!(
        completions["result"][2]["textEdit"]["range"]["start"]["character"],
        0
    );
}

fn assert_list_completion_and_continuation(lsp: &mut LspProcess) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 3},
            "contentChanges": [{"text": "*"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "textDocument/completion",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "position": {"line": 0, "character": 1},
            "context": {"triggerKind": 2, "triggerCharacter": "*"}
        }
    }));
    let completions = lsp.receive();
    assert_eq!(completions["id"], 6);
    assert_eq!(completions["result"][0]["label"], "*");
    assert_eq!(completions["result"][2]["label"], "***");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 4},
            "contentChanges": [{"text": "** item\n"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "textDocument/onTypeFormatting",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "position": {"line": 1, "character": 0},
            "ch": "\n",
            "options": {"tabSize": 4, "insertSpaces": true}
        }
    }));
    let continuation = lsp.receive();
    assert_eq!(continuation["id"], 7);
    assert_eq!(continuation["result"][0]["newText"], "** ");
}

fn assert_list_hierarchy_changes(lsp: &mut LspProcess) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 5},
            "contentChanges": [{"text": "** item"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 6},
            "contentChanges": [{"text": "** item\t"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "textDocument/onTypeFormatting",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "position": {"line": 0, "character": 8},
            "ch": "\t",
            "options": {"tabSize": 4, "insertSpaces": true}
        }
    }));
    let indent = lsp.receive();
    assert_eq!(indent["id"], 8);
    assert_eq!(indent["result"][0]["newText"], "");
    assert_eq!(indent["result"][1]["newText"], "*");

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 7},
            "contentChanges": [{"text": "*** item"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 8},
            "contentChanges": [{"text": "*** item\u{000b}"}]
        }
    }));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "textDocument/onTypeFormatting",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "position": {"line": 0, "character": 9},
            "ch": "\u{000b}",
            "options": {"tabSize": 4, "insertSpaces": true}
        }
    }));
    let outdent = lsp.receive();
    assert_eq!(outdent["id"], 9);
    assert_eq!(outdent["result"][1]["range"]["start"]["character"], 0);
    assert_eq!(outdent["result"][1]["range"]["end"]["character"], 1);
}

fn assert_copy_heading_actions(lsp: &mut LspProcess) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "range": {
                "start": {"line": 3, "character": 0},
                "end": {"line": 3, "character": 0}
            },
            "context": {"diagnostics": []}
        }
    }));
    let code_actions = lsp.receive();
    assert_eq!(code_actions["id"], 10);
    assert_eq!(
        code_actions["result"][0]["title"],
        "Copy heading title: Child"
    );
    assert_eq!(
        code_actions["result"][0]["command"],
        "fswiki.copyHeadingTitle"
    );
    assert_eq!(code_actions["result"][0]["arguments"], json!(["Child"]));
    assert_eq!(
        code_actions["result"][1]["title"],
        "Copy heading path: [Root] > [Child]"
    );
    assert_eq!(
        code_actions["result"][1]["command"],
        "fswiki.copyHeadingPath"
    );
    assert_eq!(
        code_actions["result"][1]["arguments"],
        json!(["[Root] > [Child]"])
    );
    assert_eq!(
        code_actions["result"][2]["title"],
        "Copy heading section: Child"
    );
    assert_eq!(
        code_actions["result"][2]["command"],
        "fswiki.copyHeadingSection"
    );
    assert_eq!(
        code_actions["result"][2]["arguments"],
        json!(["!! Child\ntext\n"])
    );
    assert_eq!(
        code_actions["result"][3]["title"],
        "Copy heading section with ancestors and siblings: Child"
    );
    assert_eq!(
        code_actions["result"][3]["command"],
        "fswiki.copyHeadingSectionWithAncestorsAndSiblings"
    );
    assert_eq!(
        code_actions["result"][3]["arguments"],
        json!(["!!! Root\n, aaa,bb\n!! Child\ntext\n"])
    );
}

fn assert_initialize_response(initialized: &Value) {
    assert_eq!(initialized["id"], 1);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "fswiki-lsp");
    let capabilities = &initialized["result"]["capabilities"];
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(
        capabilities["completionProvider"]["triggerCharacters"],
        json!(["!", "{", "'", "=", "_", "*", "+"])
    );
    assert_eq!(
        capabilities["documentOnTypeFormattingProvider"]["firstTriggerCharacter"],
        "\n"
    );
    assert_eq!(
        capabilities["documentOnTypeFormattingProvider"]["moreTriggerCharacter"],
        json!(["\t", "\u{000b}"])
    );
    assert_eq!(capabilities["codeActionProvider"], true);
    assert_eq!(
        capabilities["executeCommandProvider"]["commands"],
        json!([
            "fswiki.copyHeadingTitle",
            "fswiki.copyHeadingPath",
            "fswiki.copyHeadingSection",
            "fswiki.copyHeadingSectionWithAncestorsAndSiblings"
        ])
    );
}

fn assert_published_diagnostics(lsp: &mut LspProcess) {
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {"uri": "file:///test.fsw", "version": 9},
            "contentChanges": [{"text": "!! Child\n{{ref image.png\n"}]
        }
    }));
    let published = lsp
        .receive_notification_matching("textDocument/publishDiagnostics", |message| {
            message["params"]["version"] == 9
        });
    assert_eq!(published["params"]["version"], 9);
    assert_eq!(
        published["params"]["diagnostics"][0]["code"],
        "fswiki/heading-level-jump"
    );
    assert_eq!(published["params"]["diagnostics"][0]["severity"], 2);
    assert_eq!(
        published["params"]["diagnostics"][1]["code"],
        "fswiki/unclosed-plugin"
    );
    assert_eq!(published["params"]["diagnostics"][1]["severity"], 1);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {"textDocument": {"uri": "file:///test.fsw"}}
    }));
    let cleared = lsp.receive_notification("textDocument/publishDiagnostics");
    assert_eq!(cleared["params"]["diagnostics"], json!([]));
}

#[test]
fn serves_formatting_symbols_folding_and_completion_over_stdio() {
    let mut lsp = LspProcess::spawn();
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "capabilities": {},
            "initializationOptions": {
                "formatting": {
                    "tableAlign": "left",
                    "tableCellSuffixSpace": true
                }
            }
        }
    }));
    let initialized = lsp.receive();
    assert_initialize_response(&initialized);

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///test.fsw",
                "languageId": "fswiki",
                "version": 1,
                "text": "!!! Root\n, aaa,bb\n!! Child\ntext\n"
            }
        }
    }));
    let opened_diagnostics = lsp.receive_notification("textDocument/publishDiagnostics");
    assert_eq!(opened_diagnostics["params"]["version"], 1);
    assert_eq!(opened_diagnostics["params"]["diagnostics"], json!([]));
    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/formatting",
        "params": {
            "textDocument": {"uri": "file:///test.fsw"},
            "options": {"tabSize": 4, "insertSpaces": true}
        }
    }));
    let formatting = lsp.receive();
    assert_eq!(formatting["id"], 2);
    assert_eq!(
        formatting["result"][0]["newText"],
        "!!! Root\n\n,aaa ,bb\n\n!! Child\n\ntext\n"
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/documentSymbol",
        "params": {"textDocument": {"uri": "file:///test.fsw"}}
    }));
    let symbols = lsp.receive();
    assert_eq!(symbols["id"], 3);
    assert_eq!(symbols["result"][0]["name"], "Root");
    assert_eq!(symbols["result"][0]["children"][0]["name"], "Child");
    assert_eq!(symbols["result"][0]["range"]["end"]["line"], 4);
    assert_eq!(
        symbols["result"][0]["selectionRange"],
        json!({
            "start": {"line": 0, "character": 4},
            "end": {"line": 0, "character": 8}
        })
    );

    lsp.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "textDocument/foldingRange",
        "params": {"textDocument": {"uri": "file:///test.fsw"}}
    }));
    let folds = lsp.receive();
    assert_eq!(folds["id"], 4);
    assert_eq!(folds["result"][0]["startLine"], 0);

    assert_copy_heading_actions(&mut lsp);

    assert_plugin_completion(&mut lsp);
    assert_list_completion_and_continuation(&mut lsp);
    assert_list_hierarchy_changes(&mut lsp);
    assert_published_diagnostics(&mut lsp);

    lsp.stop();
}
