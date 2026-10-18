/* Prints the plusargs in the argv `vpi_get_vlog_info` returns (IEEE
 * 1800-2017 §38.17), read at cbStartOfSimulation the way cocotb reads them,
 * so a test can check which plusargs a VPI client sees. */
#include <stdio.h>
#include <string.h>

#include "vpi_user.h"

static PLI_INT32 start_of_sim(p_cb_data cb) {
    s_vpi_vlog_info info;
    int i;
    (void)cb;
    memset(&info, 0, sizeof info);
    if (!vpi_get_vlog_info(&info)) {
        printf("VPI_ARGV|failed\n");
        return 0;
    }
    for (i = 0; i < info.argc; i++) {
        if (info.argv[i] && info.argv[i][0] == '+') {
            printf("VPI_PLUSARG|%s\n", info.argv[i]);
        }
    }
    printf("VPI_ARGC|%d\n", info.argc);
    fflush(stdout);
    return 0;
}

static void register_start_of_sim(void) {
    s_cb_data cb;
    memset(&cb, 0, sizeof cb);
    cb.reason = cbStartOfSimulation;
    cb.cb_rtn = start_of_sim;
    vpi_register_cb(&cb);
}

void (*vlog_startup_routines[])(void) = {register_start_of_sim, 0};
