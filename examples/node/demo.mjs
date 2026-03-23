/**
 * envproxy Node.js demo — live environment variable monitoring.
 *
 * Node.js calls libc getenv() on every process.env access,
 * so LD_PRELOAD interception works out of the box — no patching needed.
 *
 * Usage:
 *   mise run demo:node
 */

const KEYS = ["DATABASE_URL", "API_KEY", "REDIS_URL", "JWT_SECRET"];

function mask(value) {
  if (!value) return "<not set>";
  if (value.length <= 12) return value;
  return value.slice(0, 6) + "..." + value.slice(-6);
}

function ts() {
  return new Date().toLocaleTimeString("en-GB", { hour12: false });
}

console.log("envproxy Node.js demo");
console.log(
  "Edit examples/secrets.json to see live rotation. Ctrl+C to stop.\n"
);

setInterval(() => {
  const now = ts();
  for (const key of KEYS) {
    console.log(`  [${now}] ${key} = ${mask(process.env[key])}`);
  }
  console.log();
}, 3000);
