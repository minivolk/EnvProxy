/**
 * envproxy Java integration test.
 *
 * Java's System.getenv() calls libc getenv() via JNI,
 * so LD_PRELOAD interception works out of the box.
 *
 * Usage:
 *   javac -d /tmp/envproxy examples/TestJava.java
 *   LD_PRELOAD=target/release/libenvproxy.so java -cp /tmp/envproxy TestJava
 */
public class TestJava {

    static int passed = 0;
    static int failed = 0;

    static void check(String key, String expected) {
        String actual = System.getenv(key);
        if (expected.equals(actual)) {
            passed++;
        } else {
            System.err.println("FAIL: " + key + " = \"" + actual + "\" (expected \"" + expected + "\")");
            failed++;
        }
    }

    static void checkNull(String key) {
        String actual = System.getenv(key);
        if (actual == null) {
            passed++;
        } else {
            System.err.println("FAIL: " + key + " = \"" + actual + "\" (expected null)");
            failed++;
        }
    }

    static void checkNotEmpty(String key) {
        String actual = System.getenv(key);
        if (actual != null && !actual.isEmpty()) {
            passed++;
        } else {
            System.err.println("FAIL: " + key + " = \"" + actual + "\" (expected non-empty)");
            failed++;
        }
    }

    public static void main(String[] args) {
        check("DATABASE_URL", "postgres://user:secret@localhost:5432/mydb");
        check("API_KEY", "sk-envproxy-demo-1234567890");
        check("REDIS_URL", "redis://localhost:6379/0");
        check("JWT_SECRET", "super-secret-jwt-signing-key");

        // Missing key should return null.
        checkNull("NONEXISTENT_KEY_12345");

        // Real env vars should still work.
        checkNotEmpty("HOME");

        if (failed > 0) {
            System.err.println("Java: " + passed + " passed, " + failed + " failed");
            System.exit(1);
        } else {
            System.out.println("Java: " + passed + " passed");
        }
    }
}
