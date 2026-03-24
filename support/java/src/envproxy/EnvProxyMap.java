package envproxy;

import java.io.InputStream;
import java.io.OutputStream;
import java.net.StandardProtocolFamily;
import java.net.UnixDomainSocketAddress;
import java.nio.ByteBuffer;
import java.nio.channels.SocketChannel;
import java.util.*;

/**
 * A read-only Map&lt;String, String&gt; proxy that wraps the original JVM
 * environment map and queries the envproxy-agent for missing keys.
 *
 * <p>This map is installed as a replacement for
 * {@code ProcessEnvironment.theUnmodifiableEnvironment} by {@link EnvProxyAgent}.
 *
 * <p>Behavior:
 * <ul>
 *   <li>{@code get(key)}: returns from original map if present, otherwise queries agent</li>
 *   <li>{@code containsKey(key)}: checks original, then agent</li>
 *   <li>All mutating operations throw {@link UnsupportedOperationException}
 *       (matches the original unmodifiable map behavior)</li>
 *   <li>Resolved values are cached with a configurable TTL</li>
 * </ul>
 */
public class EnvProxyMap extends AbstractMap<String, String> {

    private static final byte PROTOCOL_VERSION = 1;
    private static final byte STATUS_FOUND = 0x00;

    private final Map<String, String> original;
    private final String socketPath;
    private final long cacheTtlMs;
    private final Map<String, CacheEntry> cache = new HashMap<>();

    private static final class CacheEntry {
        final String value; // null means "not found"
        final long timestamp;

        CacheEntry(String value, long timestamp) {
            this.value = value;
            this.timestamp = timestamp;
        }
    }

    public EnvProxyMap(Map<String, String> original, String socketPath, long cacheTtlMs) {
        this.original = original;
        this.socketPath = socketPath;
        this.cacheTtlMs = cacheTtlMs;
    }

    @Override
    public String get(Object key) {
        // Fast path: check original map.
        String value = original.get(key);
        if (value != null) {
            return value;
        }
        // original.get() returns null for both "key not present" and (theoretically)
        // "key mapped to null". Use containsKey to distinguish, but the JVM env
        // never has null values, so we can skip that check.

        if (!(key instanceof String)) {
            return null;
        }
        String keyStr = (String) key;

        // Check cache.
        synchronized (cache) {
            CacheEntry entry = cache.get(keyStr);
            if (entry != null && (System.currentTimeMillis() - entry.timestamp) < cacheTtlMs) {
                return entry.value;
            }
        }

        // Query agent.
        String resolved = queryAgent(keyStr);
        synchronized (cache) {
            cache.put(keyStr, new CacheEntry(resolved, System.currentTimeMillis()));
        }
        return resolved;
    }

    @Override
    public boolean containsKey(Object key) {
        if (original.containsKey(key)) {
            return true;
        }
        return get(key) != null;
    }

    @Override
    public Set<Entry<String, String>> entrySet() {
        // Delegate to original — we can't enumerate agent keys.
        return original.entrySet();
    }

    @Override
    public int size() {
        return original.size();
    }

    @Override
    public boolean isEmpty() {
        return original.isEmpty();
    }

    @Override
    public Set<String> keySet() {
        return original.keySet();
    }

    @Override
    public Collection<String> values() {
        return original.values();
    }

    // Mutation operations — throw like the original unmodifiable map.

    @Override
    public String put(String key, String value) {
        throw new UnsupportedOperationException();
    }

    @Override
    public String remove(Object key) {
        throw new UnsupportedOperationException();
    }

    @Override
    public void clear() {
        throw new UnsupportedOperationException();
    }

    /**
     * Query the envproxy-agent via Unix domain socket.
     *
     * <p>Wire protocol:
     * <pre>
     * Request:  [1: version] [2: key_len BE] [N: key]
     * Response: [1: status]  [2: val_len BE] [N: value]
     * </pre>
     */
    private String queryAgent(String key) {
        try {
            byte[] keyBytes = key.getBytes(java.nio.charset.StandardCharsets.UTF_8);
            if (keyBytes.length > 0xFFFF) {
                return null;
            }

            UnixDomainSocketAddress addr = UnixDomainSocketAddress.of(socketPath);
            try (SocketChannel channel = SocketChannel.open(StandardProtocolFamily.UNIX)) {
                channel.connect(addr);

                // Build request.
                ByteBuffer request = ByteBuffer.allocate(1 + 2 + keyBytes.length);
                request.put(PROTOCOL_VERSION);
                request.putShort((short) keyBytes.length);
                request.put(keyBytes);
                request.flip();
                channel.write(request);

                // Read response header.
                ByteBuffer header = ByteBuffer.allocate(3);
                readFully(channel, header);
                header.flip();
                byte status = header.get();
                int valLen = Short.toUnsignedInt(header.getShort());

                // Read value.
                byte[] valueBytes = new byte[valLen];
                if (valLen > 0) {
                    ByteBuffer valBuf = ByteBuffer.wrap(valueBytes);
                    readFully(channel, valBuf);
                }

                if (status == STATUS_FOUND) {
                    return new String(valueBytes, java.nio.charset.StandardCharsets.UTF_8);
                }
                return null;
            }
        } catch (Exception e) {
            return null;
        }
    }

    private static void readFully(SocketChannel channel, ByteBuffer buf) throws java.io.IOException {
        while (buf.hasRemaining()) {
            if (channel.read(buf) < 0) {
                throw new java.io.IOException("unexpected EOF");
            }
        }
    }
}
