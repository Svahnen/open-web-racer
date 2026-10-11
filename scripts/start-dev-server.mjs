import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";
import { spawn, spawnSync } from "node:child_process";

const root = process.cwd();
const cargoBin = join(homedir(), ".cargo", "bin");
const cargo = process.env.CARGO ?? join(cargoBin, process.platform === "win32" ? "cargo.exe" : "cargo");
const wasmBindgen = join(cargoBin, process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen");
// Builds and starts the server plus the WASM client. Other server/client packages that expose the
// same interface can be selected with RACER_SERVER_PACKAGE and RACER_CLIENT_PACKAGE.
const serverPackage = process.env.RACER_SERVER_PACKAGE ?? "racer-server";
const clientPackage = process.env.RACER_CLIENT_PACKAGE ?? "racer-client";
const output = join(root, "target", "wasm32-unknown-unknown", "release", `${clientPackage.replace(/-/g, "_")}.wasm`);
const env = { ...process.env, PATH: `${cargoBin}${delimiter}${process.env.PATH ?? ""}` };

function run(executable, args) {
  const result = spawnSync(executable, args, { cwd: root, env, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (!existsSync(wasmBindgen)) throw new Error(`wasm-bindgen was not found at ${wasmBindgen}`);
run(cargo, ["build", "-p", serverPackage]);
run(cargo, ["build", "-p", clientPackage, "--target", "wasm32-unknown-unknown", "--release"]);
run(wasmBindgen, [output, "--target", "web", "--out-dir", join(root, "web", "pkg"), "--out-name", "racer_client"]);

const serverName = serverPackage;
const server = join(root, "target", "debug", process.platform === "win32" ? `${serverName}.exe` : serverName);
const child = spawn(server, [], { cwd: root, env, stdio: "inherit" });
let closing = false;
function shutdown() {
  if (closing) return;
  closing = true;
  child.kill();
  setTimeout(() => process.exit(0), 500).unref();
}
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
child.on("error", (error) => { console.error(error); process.exit(1); });
child.on("exit", (code) => process.exit(code ?? 0));
