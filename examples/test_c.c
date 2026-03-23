/**
 * envproxy C integration test.
 *
 * Directly tests the LD_PRELOAD getenv() interception.
 * This is the most fundamental test — if this works, the .so is correct.
 *
 * Usage:
 *   cc -o /tmp/envproxy/test_c examples/test_c.c
 *   LD_PRELOAD=target/release/libenvproxy.so /tmp/envproxy/test_c
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int passed = 0;
static int failed = 0;

static void check(const char *key, const char *expected) {
    const char *actual = getenv(key);
    if (actual != NULL && strcmp(actual, expected) == 0) {
        passed++;
    } else {
        fprintf(stderr, "FAIL: %s = \"%s\" (expected \"%s\")\n",
                key, actual ? actual : "(null)", expected);
        failed++;
    }
}

static void check_null(const char *key) {
    const char *actual = getenv(key);
    if (actual == NULL) {
        passed++;
    } else {
        fprintf(stderr, "FAIL: %s = \"%s\" (expected NULL)\n", key, actual);
        failed++;
    }
}

static void check_not_null(const char *key) {
    const char *actual = getenv(key);
    if (actual != NULL && strlen(actual) > 0) {
        passed++;
    } else {
        fprintf(stderr, "FAIL: %s = \"%s\" (expected non-empty)\n",
                key, actual ? actual : "(null)");
        failed++;
    }
}

int main(void) {
    check("DATABASE_URL", "postgres://user:secret@localhost:5432/mydb");
    check("API_KEY", "sk-envproxy-demo-1234567890");
    check("REDIS_URL", "redis://localhost:6379/0");
    check("JWT_SECRET", "super-secret-jwt-signing-key");

    /* Missing key should return NULL. */
    check_null("NONEXISTENT_KEY_12345");

    /* Real env vars should still work. */
    check_not_null("HOME");

    if (failed > 0) {
        fprintf(stderr, "C: %d passed, %d failed\n", passed, failed);
        return 1;
    }
    printf("C: %d passed\n", passed);
    return 0;
}
