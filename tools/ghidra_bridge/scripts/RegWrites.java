// Lists the instructions in the function containing <addr> that write register <reg>.
// Usage: ... -postScript RegWrites.java <hex addr> <REG> <out file>
//@category OpenMXBikes

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

public class RegWrites extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        Function f = getFunctionContaining(toAddr(Long.parseUnsignedLong(args[0], 16)));
        String reg = args[1].toUpperCase();
        StringBuilder sb = new StringBuilder();
        Instruction ins = getInstructionAt(f.getEntryPoint());
        Instruction prev = null;
        while (ins != null && f.getBody().contains(ins.getAddress())) {
            String text = ins.toString();
            int comma = text.indexOf(',');
            String dst = comma > 0 ? text.substring(0, comma) : text;
            if (dst.endsWith(" " + reg)) {
                if (prev != null) {
                    sb.append(prev.getAddress()).append("    ").append(prev).append('\n');
                }
                sb.append(ins.getAddress()).append("  > ").append(text).append('\n');
            }
            prev = ins;
            ins = ins.getNext();
        }
        Files.write(new File(args[2]).toPath(), sb.toString().getBytes(StandardCharsets.UTF_8));
    }
}
