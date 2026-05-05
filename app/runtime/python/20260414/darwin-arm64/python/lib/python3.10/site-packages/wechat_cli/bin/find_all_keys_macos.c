/*
 * find_all_keys_macos.c - macOS WeChat memory key scanner
 *
 * Scans WeChat process memory for SQLCipher encryption keys in the
 * x'<key_hex><salt_hex>' format used by WeChat 4.x on macOS.
 *
 * Prerequisites:
 *   - WeChat must be ad-hoc signed (or SIP disabled)
 *   - Must run as root (sudo)
 *
 * Build:
 *   cc -O2 -o find_all_keys_macos find_all_keys_macos.c -framework Foundation
 *
 * Usage:
 *   sudo ./find_all_keys_macos [pid]
 *   If pid is omitted, automatically finds WeChat PID.
 *
 * Output: JSON file at ./all_keys.json (compatible with decrypt_db.py)
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <dirent.h>
#include <ctype.h>
#include <ftw.h>
#include <pwd.h>
#include <sys/stat.h>
#include <mach/mach.h>
#include <mach/mach_vm.h>

#define MAX_KEYS 256
#define MAX_RAW_KEYS 8192
#define KEY_SIZE 32
#define SALT_SIZE 16
#define HEX_PATTERN_LEN 96  /* 64 hex (key) + 32 hex (salt) */
#define RAW_KEY_HEX_LEN 64
#define CHUNK_SIZE (2 * 1024 * 1024)

typedef struct {
    char key_hex[65];
    char salt_hex[33];
    char full_pragma[100];
} key_entry_t;

static char g_raw_key_hex[MAX_RAW_KEYS][65];
static int g_raw_key_count = 0;

/* Forward declaration */
static int read_db_salt(const char *path, char *salt_hex_out);

/* nftw callback state for collecting DB files */
#define MAX_DBS 256
static char g_db_salts[MAX_DBS][33];
static char g_db_names[MAX_DBS][512];
static off_t g_db_sizes[MAX_DBS];
static int g_db_count = 0;
static int nftw_collect_db(const char *fpath, const struct stat *sb,
                           int typeflag, struct FTW *ftwbuf) {
    (void)ftwbuf;
    if (typeflag != FTW_F) return 0;
    size_t len = strlen(fpath);
    if (len < 3 || strcmp(fpath + len - 3, ".db") != 0) return 0;
    if (g_db_count >= MAX_DBS) return 0;

    char salt[33];
    if (read_db_salt(fpath, salt) != 0) return 0;

    strcpy(g_db_salts[g_db_count], salt);
    /* Extract relative path from db_storage/ */
    const char *rel = strstr(fpath, "db_storage/");
    if (rel) rel += strlen("db_storage/");
    else {
        rel = strrchr(fpath, '/');
        rel = rel ? rel + 1 : fpath;
    }
    strncpy(g_db_names[g_db_count], rel, sizeof(g_db_names[g_db_count]) - 1);
    g_db_names[g_db_count][sizeof(g_db_names[g_db_count]) - 1] = '\0';
    g_db_sizes[g_db_count] = sb ? sb->st_size : 0;
    printf("  %s: salt=%s\n", g_db_names[g_db_count], salt);
    g_db_count++;
    return 0;
}

static int is_hex_char(unsigned char c) {
    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

static void lowercase_hex(char *s) {
    for (int j = 0; s[j]; j++)
        if (s[j] >= 'A' && s[j] <= 'F')
            s[j] += 32;
}

static void add_raw_key_candidate(const char *key_hex) {
    for (int k = 0; k < g_raw_key_count; k++) {
        if (strcmp(g_raw_key_hex[k], key_hex) == 0)
            return;
    }
    if (g_raw_key_count < MAX_RAW_KEYS) {
        strcpy(g_raw_key_hex[g_raw_key_count], key_hex);
        g_raw_key_count++;
    }
}

static pid_t find_wechat_pid(void) {
    FILE *fp = popen("pgrep -x WeChat", "r");
    if (!fp) return -1;
    char buf[64];
    pid_t pid = -1;
    if (fgets(buf, sizeof(buf), fp))
        pid = atoi(buf);
    pclose(fp);
    return pid;
}

static int is_decimal_arg(const char *s) {
    if (!s || !*s) return 0;
    for (const char *p = s; *p; p++)
        if (!isdigit((unsigned char)*p)) return 0;
    return 1;
}

static void print_usage(const char *prog) {
    fprintf(stderr,
        "Usage: sudo %s [--pid PID] [--db-dir DB_STORAGE_DIR] [--out all_keys.json]\n"
        "       sudo %s [PID]\n",
        prog, prog);
}

static void write_json_string(FILE *fp, const char *s) {
    fputc('"', fp);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        if (*p == '"' || *p == '\\') {
            fputc('\\', fp);
            fputc(*p, fp);
        } else if (*p == '\n') {
            fputs("\\n", fp);
        } else if (*p == '\r') {
            fputs("\\r", fp);
        } else if (*p == '\t') {
            fputs("\\t", fp);
        } else if (*p < 0x20) {
            fprintf(fp, "\\u%04x", *p);
        } else {
            fputc(*p, fp);
        }
    }
    fputc('"', fp);
}

/* Read DB salt (first 16 bytes) and return hex string */
static int read_db_salt(const char *path, char *salt_hex_out) {
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    unsigned char header[16];
    if (fread(header, 1, 16, f) != 16) { fclose(f); return -1; }
    fclose(f);
    /* Check if unencrypted */
    if (memcmp(header, "SQLite format 3", 15) == 0) return -1;
    for (int i = 0; i < 16; i++)
        sprintf(salt_hex_out + i * 2, "%02x", header[i]);
    salt_hex_out[32] = '\0';
    return 0;
}

int main(int argc, char *argv[]) {
    pid_t pid = -1;
    const char *db_dir = NULL;
    const char *out_path = "all_keys.json";

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--pid") == 0) {
            if (++i >= argc) { print_usage(argv[0]); return 2; }
            pid = atoi(argv[i]);
        } else if (strcmp(argv[i], "--db-dir") == 0) {
            if (++i >= argc) { print_usage(argv[0]); return 2; }
            db_dir = argv[i];
        } else if (strcmp(argv[i], "--out") == 0) {
            if (++i >= argc) { print_usage(argv[0]); return 2; }
            out_path = argv[i];
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            return 0;
        } else if (is_decimal_arg(argv[i]) && pid <= 0) {
            /* Backward compatible form: find_all_keys_macos PID */
            pid = atoi(argv[i]);
        } else if (!db_dir) {
            /* Convenience form for manual runs: find_all_keys_macos /path/to/db_storage */
            db_dir = argv[i];
        } else {
            fprintf(stderr, "Unknown argument: %s\n", argv[i]);
            print_usage(argv[0]);
            return 2;
        }
    }

    if (pid <= 0)
        pid = find_wechat_pid();

    if (pid <= 0) {
        fprintf(stderr, "WeChat not running or invalid PID\n");
        return 1;
    }

    printf("============================================================\n");
    printf("  macOS WeChat Memory Key Scanner (C version)\n");
    printf("============================================================\n");
    printf("WeChat PID: %d\n", pid);

    /* Get task port */
    mach_port_t task;
    kern_return_t kr = task_for_pid(mach_task_self(), pid, &task);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid failed: %d\n", kr);
        fprintf(stderr, "Make sure: (1) running as root, (2) WeChat is ad-hoc signed\n");
        return 1;
    }
    printf("Got task port: %u\n", task);

    /* Resolve real user's HOME (sudo may change HOME to /var/root) */
    const char *home = getenv("HOME");
    const char *sudo_user = getenv("SUDO_USER");
    if (sudo_user) {
        struct passwd *pw = getpwnam(sudo_user);
        if (pw && pw->pw_dir)
            home = pw->pw_dir;
    }
    if (!home) home = "/root";
    printf("User home: %s\n", home);

    /* Collect DB salts by recursively walking db_storage directories.
     * Note: POSIX glob() does not support ** recursive matching on macOS,
     * so we use nftw() to walk the directory tree instead. */
    printf("\nScanning for DB files...\n");
    if (db_dir) {
        struct stat st;
        if (stat(db_dir, &st) != 0 || !S_ISDIR(st.st_mode)) {
            fprintf(stderr, "DB directory does not exist: %s\n", db_dir);
            return 1;
        }
        printf("DB dir: %s\n", db_dir);
        nftw(db_dir, nftw_collect_db, 20, FTW_PHYS);
    } else {
        char db_base_dir[512];
        snprintf(db_base_dir, sizeof(db_base_dir),
            "%s/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files",
            home);

        /* Walk each account's db_storage directory */
        DIR *xdir = opendir(db_base_dir);
        if (xdir) {
            struct dirent *ent;
            while ((ent = readdir(xdir)) != NULL) {
                if (ent->d_name[0] == '.') continue;
                char storage_path[768];
                snprintf(storage_path, sizeof(storage_path),
                    "%s/%s/db_storage", db_base_dir, ent->d_name);
                struct stat st;
                if (stat(storage_path, &st) == 0 && S_ISDIR(st.st_mode)) {
                    nftw(storage_path, nftw_collect_db, 20, FTW_PHYS);
                }
            }
            closedir(xdir);
        }
    }
    printf("Found %d encrypted DBs\n", g_db_count);

    /* Scan memory for x' patterns */
    printf("\nScanning memory for keys...\n");
    key_entry_t keys[MAX_KEYS];
    int key_count = 0;
    size_t total_scanned = 0;
    int region_count = 0;

    mach_vm_address_t addr = 0;
    while (1) {
        mach_vm_size_t size = 0;
        vm_region_basic_info_data_64_t info;
        mach_msg_type_number_t info_count = VM_REGION_BASIC_INFO_COUNT_64;
        mach_port_t obj_name;

        kr = mach_vm_region(task, &addr, &size, VM_REGION_BASIC_INFO_64,
                           (vm_region_info_t)&info, &info_count, &obj_name);
        if (kr != KERN_SUCCESS) break;
        if (size == 0) { addr++; continue; }  /* guard against infinite loop */

        if ((info.protection & (VM_PROT_READ | VM_PROT_WRITE)) ==
            (VM_PROT_READ | VM_PROT_WRITE)) {
            region_count++;

            mach_vm_address_t ca = addr;
            while (ca < addr + size) {
                mach_vm_size_t cs = addr + size - ca;
                if (cs > CHUNK_SIZE) cs = CHUNK_SIZE;

                vm_offset_t data;
                mach_msg_type_number_t dc;
                kr = mach_vm_read(task, ca, cs, &data, &dc);
                if (kr == KERN_SUCCESS) {
                    unsigned char *buf = (unsigned char *)data;
                    total_scanned += dc;

                    for (size_t i = 0; i < dc; i++) {
                        if (i + HEX_PATTERN_LEN + 3 < dc &&
                            buf[i] == 'x' && buf[i + 1] == '\'') {
                            /* Check if followed by 96 hex chars and closing ' */
                            int valid = 1;
                            for (int j = 0; j < HEX_PATTERN_LEN; j++) {
                                if (!is_hex_char(buf[i + 2 + j])) { valid = 0; break; }
                            }
                            if (!valid) continue;
                            if (buf[i + 2 + HEX_PATTERN_LEN] != '\'') continue;

                            /* Extract key and salt hex */
                            char key_hex[65], salt_hex[33];
                            memcpy(key_hex, buf + i + 2, 64);
                            key_hex[64] = '\0';
                            memcpy(salt_hex, buf + i + 2 + 64, 32);
                            salt_hex[32] = '\0';

                            /* Convert to lowercase for comparison */
                            lowercase_hex(key_hex);
                            lowercase_hex(salt_hex);
                            add_raw_key_candidate(key_hex);

                            /* Deduplicate */
                            int dup = 0;
                            for (int k = 0; k < key_count; k++) {
                                if (strcmp(keys[k].key_hex, key_hex) == 0 &&
                                    strcmp(keys[k].salt_hex, salt_hex) == 0) {
                                    dup = 1; break;
                                }
                            }
                            if (dup) continue;

                            if (key_count < MAX_KEYS) {
                                strcpy(keys[key_count].key_hex, key_hex);
                                strcpy(keys[key_count].salt_hex, salt_hex);
                                snprintf(keys[key_count].full_pragma, sizeof(keys[key_count].full_pragma),
                                    "x'%s%s'", key_hex, salt_hex);
                                key_count++;
                            }
                        }

                        /* Some WeChat builds keep the 64-hex SQLCipher key in
                         * memory without the x'<key><salt>' wrapper. Collect
                         * exact-length candidates; Python verifies them later. */
                        if (i + RAW_KEY_HEX_LEN <= dc) {
                            int valid_raw = 1;
                            for (int j = 0; j < RAW_KEY_HEX_LEN; j++) {
                                if (!is_hex_char(buf[i + j])) { valid_raw = 0; break; }
                            }
                            if (valid_raw) {
                                int prev_hex = (i > 0 && is_hex_char(buf[i - 1]));
                                int next_hex = (i + RAW_KEY_HEX_LEN < dc && is_hex_char(buf[i + RAW_KEY_HEX_LEN]));
                                if (!prev_hex && !next_hex) {
                                    char raw_key[65];
                                    memcpy(raw_key, buf + i, RAW_KEY_HEX_LEN);
                                    raw_key[64] = '\0';
                                    lowercase_hex(raw_key);
                                    add_raw_key_candidate(raw_key);
                                    i += RAW_KEY_HEX_LEN - 1;
                                }
                            }
                        }
                    }
                    mach_vm_deallocate(mach_task_self(), data, dc);
                }
                /* Advance with overlap to catch patterns spanning chunk boundaries.
                 * Pattern is x'<96 hex chars>' = 99 bytes total. */
                if (cs > HEX_PATTERN_LEN + 3)
                    ca += cs - (HEX_PATTERN_LEN + 3);
                else
                    ca += cs;
            }
        }
        addr += size;
    }

    printf("\nScan complete: %zuMB scanned, %d regions, %d unique keys\n",
           total_scanned / 1024 / 1024, region_count, key_count);
    printf("Collected %d raw 64-hex key candidates for verification\n", g_raw_key_count);

    /* Match keys to DBs */
    printf("\n%-25s %-66s %s\n", "Database", "Key", "Salt");
    printf("%-25s %-66s %s\n",
        "-------------------------",
        "------------------------------------------------------------------",
        "--------------------------------");

    int matched = 0;
    for (int j = 0; j < g_db_count; j++) {
        const char *key = NULL;
        for (int i = 0; i < key_count; i++) {
            if (strcmp(keys[i].salt_hex, g_db_salts[j]) == 0) {
                key = keys[i].key_hex;
                break;
            }
        }
        if (key) matched++;
        printf("%-25s %-66s %s\n",
            g_db_names[j],
            key ? key : "(missing)",
            g_db_salts[j]);
    }
    printf("\nMatched %d/%d DBs to scanned keys\n", matched, g_db_count);

    /* Save JSON: { "rel/path.db": { "enc_key": "hex" }, ... }
     * Uses forward slashes (native macOS paths, valid JSON without escaping).
     */
    FILE *fp = fopen(out_path, "w");
    if (fp) {
        fprintf(fp, "{\n");
        int first = 1;
        for (int j = 0; j < g_db_count; j++) {
            const char *key = NULL;
            for (int i = 0; i < key_count; i++) {
                if (strcmp(keys[i].salt_hex, g_db_salts[j]) == 0) {
                    key = keys[i].key_hex;
                    break;
                }
            }
            if (!key) continue;
            fprintf(fp, "%s  ", first ? "" : ",\n");
            write_json_string(fp, g_db_names[j]);
            fprintf(fp,
                ": {\"enc_key\": \"%s\", \"salt\": \"%s\", \"size_mb\": %.1f}",
                key,
                g_db_salts[j],
                (double)g_db_sizes[j] / 1024.0 / 1024.0);
            first = 0;
        }
        if (!first) fprintf(fp, ",\n");
        fprintf(fp, "  \"_raw_hex_keys\": [");
        for (int i = 0; i < g_raw_key_count; i++) {
            fprintf(fp, "%s\n    \"%s\"", i == 0 ? "" : ",", g_raw_key_hex[i]);
        }
        fprintf(fp, "\n  ]\n");
        fprintf(fp, "}\n");
        fclose(fp);
        printf("Saved to %s\n", out_path);
    }

    return 0;
}
