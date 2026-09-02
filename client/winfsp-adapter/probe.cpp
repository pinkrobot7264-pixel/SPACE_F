// Phase 0 link-check executable. If this builds and runs, the MSVC toolchain,
// the WinFsp headers and winfsp-x64.lib are all wired up correctly.

#include <stdio.h>

extern "C" const char *space_adapter_probe(void);

int main(void)
{
    printf("space winfsp adapter probe: %s\n", space_adapter_probe());
    return 0;
}
