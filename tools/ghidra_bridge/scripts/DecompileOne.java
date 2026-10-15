// Decompiles one function with a long timeout (for functions the bulk export skipped).
// Usage: analyzeHeadless <proj_dir> <proj_name> -process <program> -noanalysis
//        -scriptPath tools/ghidra_bridge/scripts -postScript DecompileOne.java <hex addr> <timeout s> <out file>
//@category OpenMXBikes

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

public class DecompileOne extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Function f = getFunctionAt(toAddr(Long.parseUnsignedLong(args[0], 16)));
        if (f == null) {
            f = getFunctionContaining(toAddr(Long.parseUnsignedLong(args[0], 16)));
        }
        if (f == null) {
            // Small engine thunks the auto-analysis left undefined (read-only project: in memory only).
            disassemble(toAddr(Long.parseUnsignedLong(args[0], 16)));
            f = createFunction(toAddr(Long.parseUnsignedLong(args[0], 16)), null);
        }
        DecompInterface d = new DecompInterface();
        DecompileOptions opts = new DecompileOptions();
        opts.setMaxPayloadMBytes(512);
        d.setOptions(opts);
        d.openProgram(currentProgram);
        DecompileResults r = d.decompileFunction(f, Integer.parseInt(args[1]), monitor);
        String c = (r != null && r.decompileCompleted()) ? r.getDecompiledFunction().getC() : ("// failed: " + (r == null ? "null" : r.getErrorMessage()));
        Files.write(new File(args[2]).toPath(), c.getBytes(StandardCharsets.UTF_8));
        println("DecompileOne wrote " + c.length() + " chars for " + f.getName());
        d.dispose();
    }
}
