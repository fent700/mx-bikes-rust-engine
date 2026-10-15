// Prints instructions from <start hex> to <end hex>.
// Usage: ... -postScript DisasmAt.java <start hex> <end hex> <out file>
//@category OpenMXBikes

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

public class DisasmAt extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Address end = toAddr(Long.parseUnsignedLong(args[1], 16));
        Instruction ins = getInstructionContaining(toAddr(Long.parseUnsignedLong(args[0], 16)));
        StringBuilder sb = new StringBuilder();
        while (ins != null && ins.getAddress().compareTo(end) <= 0) {
            sb.append(ins.getAddress()).append("  ").append(ins).append('\n');
            ins = ins.getNext();
        }
        Files.write(new File(args[2]).toPath(), sb.toString().getBytes(StandardCharsets.UTF_8));
    }
}
