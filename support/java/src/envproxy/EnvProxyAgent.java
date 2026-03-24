package envproxy;

import java.lang.instrument.Instrumentation;
import java.lang.reflect.Field;
import java.util.Map;

/**
 * Java agent that patches System.getenv() to query the envproxy-agent
 * for environment variables not found in the JVM's cached environment.
 *
 * <p>Java copies the process environment into an internal unmodifiable map
 * at JVM startup ({@code ProcessEnvironment.theUnmodifiableEnvironment}).
 * After that, {@code System.getenv()} never calls libc's getenv() again.
 *
 * <p>This agent replaces that internal map with an {@link EnvProxyMap} proxy
 * that queries the envproxy-agent Unix socket for missing keys.
 *
 * <p>Loaded automatically via {@code JAVA_TOOL_OPTIONS=-javaagent:/path/to/envproxy-agent.jar}
 * (set by the libenvproxy.so constructor).
 *
 * <p>Requires: {@code --add-opens java.base/java.lang=ALL-UNNAMED}
 */
public class EnvProxyAgent {

    public static void premain(String agentArgs, Instrumentation inst) {
        try {
            install(agentArgs);
        } catch (Exception e) {
            // Never break JVM startup — log and continue silently.
            if (System.getenv("ENVPROXY_DEBUG") != null
                    && "1".equals(System.getenv("ENVPROXY_DEBUG"))) {
                System.err.println("[envproxy-java] Failed to install: " + e.getMessage());
                e.printStackTrace(System.err);
            }
        }
    }

    public static void premain(String agentArgs) {
        premain(agentArgs, null);
    }

    @SuppressWarnings("unchecked")
    private static void install(String agentArgs) throws Exception {
        // Determine socket path.
        String socketPath = System.getenv("ENVPROXY_SOCKET");
        if (socketPath == null || socketPath.isEmpty()) {
            socketPath = "/tmp/envproxy/agent.sock";
        }

        // Check if agent socket exists.
        if (!new java.io.File(socketPath).exists()) {
            return; // Agent not running — don't patch.
        }

        // Parse optional cache TTL from agent args or env.
        long cacheTtlMs = 30_000; // default 30 seconds
        if (agentArgs != null && !agentArgs.isEmpty()) {
            try {
                cacheTtlMs = (long) (Double.parseDouble(agentArgs) * 1000);
            } catch (NumberFormatException ignored) {
            }
        } else {
            String ttlEnv = System.getenv("ENVPROXY_CACHE_TTL");
            if (ttlEnv != null) {
                try {
                    cacheTtlMs = (long) (Double.parseDouble(ttlEnv) * 1000);
                } catch (NumberFormatException ignored) {
                }
            }
        }

        // Access ProcessEnvironment via reflection.
        Class<?> peClass = Class.forName("java.lang.ProcessEnvironment");
        Field field = peClass.getDeclaredField("theUnmodifiableEnvironment");
        field.setAccessible(true);

        // Read the original map.
        Map<String, String> originalMap = (Map<String, String>) field.get(null);

        // Already patched? (Guard against double-loading.)
        if (originalMap instanceof EnvProxyMap) {
            return;
        }

        // Create proxy map.
        EnvProxyMap proxyMap = new EnvProxyMap(originalMap, socketPath, cacheTtlMs);

        // Use Unsafe to write to the static final field.
        Field unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        sun.misc.Unsafe unsafe = (sun.misc.Unsafe) unsafeField.get(null);

        long offset = unsafe.staticFieldOffset(field);
        unsafe.putObject(peClass, offset, proxyMap);
    }
}
