// Phase 0: link check only. The WinFsp callback table arrives in Phase 1
// (see docs/decisions/ADR-0001). Referencing a WinFsp symbol here forces the
// linker to resolve the import library winfsp-x64.lib.

#include <winfsp/winfsp.h>
#include <stdio.h>

extern "C" const char *space_adapter_probe(void)
{
    static char buf[64];
    UINT32 v = 0;
    FspVersion(&v);
    _snprintf_s(buf, sizeof(buf), _TRUNCATE, "winfsp %u.%u", v >> 16, v & 0xFFFF);
    return buf;
}
