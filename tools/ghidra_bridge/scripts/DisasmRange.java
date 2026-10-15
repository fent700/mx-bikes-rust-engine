// Prints, for every call to <callee> inside the function containing <caller addr>, the
// `before` instructions ahead of the call (for arguments the decompiler loses in registers).
// Usage: analyzeHeadless <proj_dir> <proj_name> -process <program> -noanalysis -readOnly
//        -scriptPath tools/ghidra_bridge/scripts -postScript DisasmRange.java <caller hex> <callee hex> <before> <out file>
//@category OpenMXBikes

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayDeque;

public class DisasmRange extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Function f = getFunctionContaining(toAddr(Long.parseUnsignedLong(args[0], 16)));
        Address callee = toAddr(Long.parseUnsignedLong(args[1], 16));
        int before = Integer.parseInt(args[2]);
        StringBuilder sb = new StringBuilder();
        Instruction ins = getInstructionAt(f.getEntryPoint());
        while (ins != null && f.getBody().contains(ins.getAddress())) {
            boolean hit = false;
            for (Address a : ins.getFlows()) {
                if (a.equals(callee)) {
                    hit = true;
                }
            }
            if (hit) {
                ArrayDeque<Instruction> back = new ArrayDeque<>();
                Instruction cur = ins;
                for (int i = 0; i < before && cur != null; i++) {
                    cur = cur.getPrevious();
                    if (cur != null) {
                        back.addFirst(cur);
                    }
                }
                sb.append("---- call at ").append(ins.getAddress()).append('\n');
                for (Instruction b : back) {
                    sb.append(b.getAddress()).append("  ").append(b).append('\n');
                }
                sb.append(ins.getAddress()).append(" > ").append(ins).append('\n');
            }
            ins = ins.getNext();
        }
        Files.write(new File(args[3]).toPath(), sb.toString().getBytes(StandardCharsets.UTF_8));
    }
}
