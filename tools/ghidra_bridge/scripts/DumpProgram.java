// Exports the analyzed program as JSONL for the Ghidra MCP bridge:
//   functions.jsonl  {"addr","name","size","callers":[addr],"callees":[addr],"c":"<pseudocode>"}
//   strings.jsonl    {"addr","value","xrefs":[func addr]}
//   data_xrefs.jsonl {"addr","func_addrs":[...]} for referenced float/double constants
// Usage: analyzeHeadless <proj_dir> <proj_name> -process mxbikes.exe -noanalysis
//        -scriptPath tools/ghidra_bridge/scripts -postScript DumpProgram.java <out_dir>
//@category OpenMXBikes

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;

import java.io.BufferedWriter;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Set;
import java.util.TreeSet;

public class DumpProgram extends GhidraScript {

    private static String esc(String s) {
        StringBuilder b = new StringBuilder(s.length() + 16);
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': b.append("\\\""); break;
                case '\\': b.append("\\\\"); break;
                case '\n': b.append("\\n"); break;
                case '\r': b.append("\\r"); break;
                case '\t': b.append("\\t"); break;
                default:
                    if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
            }
        }
        return b.toString();
    }

    private static String addrList(Set<String> addrs) {
        StringBuilder b = new StringBuilder("[");
        boolean first = true;
        for (String a : addrs) {
            if (!first) b.append(',');
            b.append('"').append(a).append('"');
            first = false;
        }
        return b.append(']').toString();
    }

    private Set<String> referencingFunctions(Address target) {
        Set<String> out = new TreeSet<>();
        ReferenceIterator it = currentProgram.getReferenceManager().getReferencesTo(target);
        while (it.hasNext()) {
            Reference r = it.next();
            Function f = getFunctionContaining(r.getFromAddress());
            if (f != null) out.add(f.getEntryPoint().toString());
        }
        return out;
    }

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args.length > 0 ? args[0] : "ghidra_dump");
        outDir.mkdirs();

        try (BufferedWriter w = Files.newBufferedWriter(new File(outDir, "strings.jsonl").toPath(), StandardCharsets.UTF_8)) {
            DataIterator it = currentProgram.getListing().getDefinedData(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Data d = it.next();
                if (!d.hasStringValue()) continue;
                Object v = d.getValue();
                if (v == null) continue;
                w.write("{\"addr\":\"" + d.getAddress() + "\",\"value\":\"" + esc(v.toString())
                        + "\",\"xrefs\":" + addrList(referencingFunctions(d.getAddress())) + "}\n");
            }
        }

        try (BufferedWriter w = Files.newBufferedWriter(new File(outDir, "data_xrefs.jsonl").toPath(), StandardCharsets.UTF_8)) {
            DataIterator it = currentProgram.getListing().getDefinedData(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Data d = it.next();
                String t = d.getDataType().getName();
                if (!t.equals("float") && !t.equals("double")) continue;
                Set<String> refs = referencingFunctions(d.getAddress());
                if (refs.isEmpty()) continue;
                w.write("{\"addr\":\"" + d.getAddress() + "\",\"type\":\"" + t + "\",\"value\":\""
                        + esc(String.valueOf(d.getValue())) + "\",\"func_addrs\":" + addrList(refs) + "}\n");
            }
        }

        DecompInterface decomp = new DecompInterface();
        decomp.setOptions(new DecompileOptions());
        decomp.openProgram(currentProgram);

        int total = currentProgram.getFunctionManager().getFunctionCount();
        int done = 0;
        try (BufferedWriter w = Files.newBufferedWriter(new File(outDir, "functions.jsonl").toPath(), StandardCharsets.UTF_8)) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                Set<String> callers = new TreeSet<>();
                for (Function c : f.getCallingFunctions(monitor)) callers.add(c.getEntryPoint().toString());
                Set<String> callees = new TreeSet<>();
                for (Function c : f.getCalledFunctions(monitor)) callees.add(c.getEntryPoint().toString());

                String code = "";
                if (!f.isThunk() && !f.isExternal()) {
                    DecompileResults res = decomp.decompileFunction(f, 60, monitor);
                    if (res != null && res.decompileCompleted() && res.getDecompiledFunction() != null) {
                        code = res.getDecompiledFunction().getC();
                    }
                }
                w.write("{\"addr\":\"" + f.getEntryPoint() + "\",\"name\":\"" + esc(f.getName())
                        + "\",\"size\":" + f.getBody().getNumAddresses()
                        + ",\"callers\":" + addrList(callers) + ",\"callees\":" + addrList(callees)
                        + ",\"c\":\"" + esc(code) + "\"}\n");
                if (++done % 500 == 0) println("decompiled " + done + "/" + total);
            }
        }
        decomp.dispose();
        println("DumpProgram complete: " + done + " functions -> " + outDir.getAbsolutePath());
    }
}
