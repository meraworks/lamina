//! Minimal MCP client over stdio for integration tests.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    pub fn start() -> Client {
        Client::start_with_env(&[])
    }

    /// Start the server with more environment variables, for example `LAMINA_LIB`.
    pub fn start_with_env(env: &[(&str, &std::path::Path)]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lamina"))
            .arg("mcp")
            .envs(env.iter().map(|(k, v)| (*k, *v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start lamina");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin,
            stdout,
            next: 1,
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        );
        assert!(init["result"]["serverInfo"]["name"] == "lamina", "{init}");
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, v: Value) {
        let mut s = serde_json::to_string(&v).unwrap();
        s.push('\n');
        self.stdin.write_all(s.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(n > 0, "server closed stdout");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v.get("id") == Some(&json!(id)) {
                return v;
            }
        }
    }

    /// Call a tool. Return (is_error, parsed JSON or raw text).
    pub fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let res = &r["result"];
        assert!(res.is_object(), "protocol error: {r}");
        let is_error = res["isError"].as_bool().unwrap_or(false);
        let text = res["content"][0]["text"].as_str().unwrap_or("").to_string();
        let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (is_error, v)
    }

    pub fn ok(&mut self, name: &str, args: Value) -> Value {
        let (err, v) = self.call(name, args.clone());
        assert!(!err, "{name}({args}) failed: {v}");
        v
    }

    pub fn tool_names(&mut self) -> Vec<String> {
        let r = self.request("tools/list", json!({}));
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
