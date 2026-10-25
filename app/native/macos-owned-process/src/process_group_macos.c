#include <libproc.h>
#include <errno.h>
#include <stdint.h>
#include <stddef.h>
#include <dlfcn.h>

#ifndef IMBOARD_PROC_LIST_SYMBOL
#define IMBOARD_PROC_LIST_SYMBOL "proc_listpgrppids"
#endif

static __typeof__(&proc_listpgrppids) group_api(void) {
    return (__typeof__(&proc_listpgrppids))dlsym(RTLD_DEFAULT, IMBOARD_PROC_LIST_SYMBOL);
}

int imboard_group_api_available(void) {
    return group_api() != NULL ? 0 : ENOSYS;
}

/* proc_listpgrppids returns PID count, not bytes. The kernel fills this list
 * under proc_list_lock across allproc and zombproc, including other UIDs and
 * in-flight forks. Only a sole waitable zombie leader establishes no descendants.
 * A full/truncated list can never establish exit; it remains safely signalable
 * while the Rust owner retains Child, and a later call can observe completion. */
int imboard_group_only_leader(int pid, int *only_leader) {
    int pids[64] = {0};
    *only_leader = 0;
    __typeof__(&proc_listpgrppids) list_group = group_api();
    if (list_group == NULL) return ENOSYS;
    errno = 0;
    int count = list_group(pid, pids, sizeof(pids));
    if (count < 0) return errno ? errno : EIO;
    if (count == 0 || count > 64) return EIO;
    if (count == 1 && pids[0] == pid) *only_leader = 1;
    return 0;
}
