/**
 * envproxy Java demo — live environment variable monitoring.
 *
 * Shows secrets being resolved dynamically from the agent.
 * Edit examples/secrets.json while this is running to see live rotation.
 *
 * Usage:
 *   # Quick start with mise:
 *   mise run demo:java
 *
 *   # Or manually:
 *   envproxy-agent --config examples/config.toml &
 *   javac -d /tmp/envproxy examples/JavaDemo.java
 *   LD_PRELOAD=target/release/libenvproxy.so java -cp /tmp/envproxy JavaDemo
 */

import java.time.LocalTime;
import java.time.format.DateTimeFormatter;

public class JavaDemo {

    private static final String[] KEYS = {
        "DATABASE_URL", "API_KEY", "REDIS_URL", "JWT_SECRET"
    };

    private static final DateTimeFormatter FMT =
        DateTimeFormatter.ofPattern("HH:mm:ss");

    public static void main(String[] args) throws InterruptedException {
        System.out.println("envproxy Java demo");
        System.out.println("Edit examples/secrets.json to see live rotation. Ctrl+C to stop.");
        System.out.println();

        while (true) {
            String ts = LocalTime.now().format(FMT);
            for (String key : KEYS) {
                String value = System.getenv(key);
                System.out.printf("  [%s] %s = %s%n", ts, key, mask(value));
            }
            System.out.println();
            Thread.sleep(1000);
        }
    }

    private static String mask(String value) {
        if (value == null) return "<not set>";
        if (value.length() <= 12) return value;
        return value.substring(0, 6) + "..." + value.substring(value.length() - 6);
    }
}
