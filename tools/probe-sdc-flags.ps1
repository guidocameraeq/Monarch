# Probes which SetDisplayConfig flag combinations Windows accepts.
#
# Every call uses SDC_VALIDATE instead of SDC_APPLY, so nothing is applied and no display
# changes: the return value is pure flag validation. Run it on any Windows machine, with any
# number of monitors, to reproduce the results quoted in force_topology_extend (apply.rs).
#
#   0  = the combination is legal (and the request is satisfiable)
#   87 = ERROR_INVALID_PARAMETER, i.e. "the combination of parameters and flags is invalid"
#   31 = ERROR_GEN_FAILURE, i.e. flags accepted, but the driver/hardware cannot satisfy it
#        (expected on a single-display machine: there is nothing to extend onto)
#
# The point of interest: 87 means the flags never reached the driver, so it is machine- and
# driver-independent. It fails the same way everywhere.

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class SdcProbe {
    [DllImport("user32.dll")]
    public static extern int SetDisplayConfig(
        uint numPathArrayElements, IntPtr pathArray,
        uint numModeInfoArrayElements, IntPtr modeInfoArray,
        uint flags);
}
"@

$VALIDATE     = 0x40    # SDC_VALIDATE           - applies nothing
$EXTEND       = 0x04    # SDC_TOPOLOGY_EXTEND
$CLONE        = 0x02    # SDC_TOPOLOGY_CLONE
$ALLOW        = 0x400   # SDC_ALLOW_CHANGES
$SAVE_DB      = 0x200   # SDC_SAVE_TO_DATABASE
$PERSIST      = 0x800   # SDC_PATH_PERSIST_IF_REQUIRED

function Probe($label, $flags) {
    $status = [SdcProbe]::SetDisplayConfig(0, [IntPtr]::Zero, 0, [IntPtr]::Zero, $flags)
    $meaning = switch ($status) {
        0     { "OK          - flags legal" }
        87    { "ILLEGAL     - ERROR_INVALID_PARAMETER (rejected at flag validation)" }
        31    { "flags legal - ERROR_GEN_FAILURE (driver/hardware cannot satisfy it)" }
        1168  { "flags legal - ERROR_NOT_FOUND (no such entry in the database)" }
        default { "status $status" }
    }
    "{0,-56} mask={1,-5} -> {2,-4} {3}" -f $label, $flags, $status, $meaning
}

Write-Output ""
Write-Output "Current code in force_topology_extend:"
Probe "VALIDATE|EXTEND|ALLOW_CHANGES|SAVE_TO_DATABASE" ($VALIDATE -bor $EXTEND -bor $ALLOW -bor $SAVE_DB)

Write-Output ""
Write-Output "Removing one flag at a time:"
Probe "VALIDATE|EXTEND|ALLOW_CHANGES|PATH_PERSIST"     ($VALIDATE -bor $EXTEND -bor $ALLOW -bor $PERSIST)
Probe "VALIDATE|EXTEND|ALLOW_CHANGES"                  ($VALIDATE -bor $EXTEND -bor $ALLOW)
Probe "VALIDATE|EXTEND|PATH_PERSIST   (proposed fix)"  ($VALIDATE -bor $EXTEND -bor $PERSIST)
Probe "VALIDATE|EXTEND"                                ($VALIDATE -bor $EXTEND)

Write-Output ""
Write-Output "SDC_ALLOW_CHANGES is the surprise - it is rejected with every SDC_TOPOLOGY_* flag,"
Write-Output "although the docs say it 'is allowed with any other valid combination':"
Probe "VALIDATE|CLONE|ALLOW_CHANGES"                   ($VALIDATE -bor $CLONE -bor $ALLOW)
Probe "VALIDATE|CLONE"                                 ($VALIDATE -bor $CLONE)
Probe "VALIDATE|CLONE|PATH_PERSIST"                    ($VALIDATE -bor $CLONE -bor $PERSIST)
Write-Output ""
