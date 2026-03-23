/**
 * envproxy Node.js integration test.
 *
 * Node.js calls getenv() on every process.env access (via libuv),
 * so LD_PRELOAD interception works out of the box — no patching needed.
 *
 * Usage:
 *   LD_PRELOAD=target/release/libenvproxy.so node examples/test_node.mjs
 */

const EXPECTED = {
  DATABASE_URL: "postgres://user:secret@localhost:5432/mydb",
  API_KEY: "sk-envproxy-demo-1234567890",
  REDIS_URL: "redis://localhost:6379/0",
  JWT_SECRET: "super-secret-jwt-signing-key",
};

let passed = 0;
let failed = 0;

for (const [key, expected] of Object.entries(EXPECTED)) {
  const actual = process.env[key];
  if (actual === expected) {
    passed++;
  } else {
    console.error(`FAIL: ${key} = "${actual}" (expected "${expected}")`);
    failed++;
  }
}

// Verify a missing key returns undefined.
const missing = process.env["NONEXISTENT_KEY_12345"];
if (missing === undefined) {
  passed++;
} else {
  console.error(`FAIL: NONEXISTENT_KEY_12345 = "${missing}" (expected undefined)`);
  failed++;
}

// Verify real env vars still work.
const home = process.env["HOME"];
if (home && home.length > 0) {
  passed++;
} else {
  console.error(`FAIL: HOME = "${home}" (expected non-empty)`);
  failed++;
}

if (failed > 0) {
  console.error(`Node.js: ${passed} passed, ${failed} failed`);
  process.exit(1);
} else {
  console.log(`Node.js: ${passed} passed`);
}
