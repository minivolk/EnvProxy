/**
 * envproxy C demo — live environment variable monitoring.
 *
 * C programs call libc getenv() directly, so LD_PRELOAD interception
 * works transparently — this is the most fundamental integration.
 *
 * Build & run:
 *   cc -o /tmp/envproxy/demo_c examples/c/demo.c
 *   mise run demo:c
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static const char *KEYS[] = {
    "DATABASE_URL", "API_KEY", "REDIS_URL", "JWT_SECRET"
};
static const int NUM_KEYS = 4;

static void print_masked(const char *key, const char *value) {
    if (value == NULL) {
        printf("  [%s] %s = <not set>\n", "", key);
        return;
    }
    size_t len = strlen(value);
    if (len <= 12) {
        printf("  [%s] %s = %s\n", "", key, value);
        return;
    }
    printf("  [%s] %s = %.6s...%s\n", "", key, value, value + len - 6);
}

int main(void) {
    printf("envproxy C demo\n");
    printf("Edit examples/secrets.json to see live rotation. Ctrl+C to stop.\n\n");

    char timebuf[16];

    while (1) {
        time_t now = time(NULL);
        struct tm *tm = localtime(&now);
        strftime(timebuf, sizeof(timebuf), "%H:%M:%S", tm);

        for (int i = 0; i < NUM_KEYS; i++) {
            const char *value = getenv(KEYS[i]);
            if (value == NULL) {
                printf("  [%s] %s = <not set>\n", timebuf, KEYS[i]);
            } else {
                size_t len = strlen(value);
                if (len <= 12) {
                    printf("  [%s] %s = %s\n", timebuf, KEYS[i], value);
                } else {
                    printf("  [%s] %s = %.6s...%s\n", timebuf, KEYS[i],
                           value, value + len - 6);
                }
            }
        }
        printf("\n");
        fflush(stdout);
        sleep(3);
    }

    return 0;
}
