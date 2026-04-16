import { describe, it, expect, vi } from "vitest";
import { ptyBus, decodeBase64, stripAnsi, extractLastLine } from "./ptyBus";

describe("decodeBase64", () => {
  it("decodes a simple ASCII base64 string", () => {
    // "hello" in base64 is "aGVsbG8="
    const result = decodeBase64("aGVsbG8=");
    expect(result).toBeInstanceOf(Uint8Array);
    expect(new TextDecoder().decode(result)).toBe("hello");
  });

  it("decodes an empty string", () => {
    const result = decodeBase64("");
    expect(result).toBeInstanceOf(Uint8Array);
    expect(result.length).toBe(0);
  });

  it("decodes binary data correctly", () => {
    // base64 for bytes [0, 1, 255]
    const result = decodeBase64("AAH/");
    expect(result[0]).toBe(0);
    expect(result[1]).toBe(1);
    expect(result[2]).toBe(255);
  });
});

describe("stripAnsi", () => {
  it("strips SGR color sequences", () => {
    expect(stripAnsi("\x1b[32mgreen text\x1b[0m")).toBe("green text");
  });

  it("strips cursor movement sequences", () => {
    expect(stripAnsi("\x1b[2Ahello\x1b[3B")).toBe("hello");
  });

  it("strips OSC title sequences (BEL terminated)", () => {
    expect(stripAnsi("\x1b]0;my terminal\x07some text")).toBe("some text");
  });

  it("strips OSC title sequences (ST terminated)", () => {
    expect(stripAnsi("\x1b]0;my terminal\x1b\\some text")).toBe("some text");
  });

  it("strips carriage returns", () => {
    expect(stripAnsi("line1\rline2")).toBe("line1line2");
  });

  it("returns plain text unchanged", () => {
    expect(stripAnsi("hello world")).toBe("hello world");
  });

  it("handles multiple ANSI sequences", () => {
    expect(stripAnsi("\x1b[1m\x1b[31mbold red\x1b[0m normal")).toBe(
      "bold red normal",
    );
  });

  it("strips charset designation sequences", () => {
    expect(stripAnsi("\x1b(Bhello\x1b(0")).toBe("hello");
  });

  it("strips private mode sequences", () => {
    expect(stripAnsi("\x1b[?25lhidden cursor\x1b[?25h")).toBe("hidden cursor");
  });
});

describe("extractLastLine", () => {
  function encode(s: string): Uint8Array {
    return new TextEncoder().encode(s);
  }

  it("extracts the last non-empty line", () => {
    expect(extractLastLine(encode("first\nsecond\nthird\n"))).toBe("third");
  });

  it("returns null for empty input", () => {
    expect(extractLastLine(encode(""))).toBeNull();
  });

  it("returns null for whitespace-only input", () => {
    expect(extractLastLine(encode("   \n  \n  "))).toBeNull();
  });

  it("strips ANSI before extracting", () => {
    expect(extractLastLine(encode("\x1b[32mcolored line\x1b[0m\n"))).toBe(
      "colored line",
    );
  });

  it("truncates lines longer than 120 characters", () => {
    const longLine = "x".repeat(200);
    const result = extractLastLine(encode(longLine));
    expect(result).toHaveLength(120);
  });

  it("handles single line without newline", () => {
    expect(extractLastLine(encode("only line"))).toBe("only line");
  });

  it("skips trailing empty lines", () => {
    expect(extractLastLine(encode("data\n\n\n"))).toBe("data");
  });
});

describe("ptyBus", () => {
  it("subscribe returns an unsubscribe function", () => {
    const listener = vi.fn();
    const unsub = ptyBus.subscribe("wt-1", listener);
    expect(typeof unsub).toBe("function");
    unsub();
  });

  it("emit delivers payloads to subscribers", () => {
    const listener = vi.fn();
    const unsub = ptyBus.subscribe("wt-1", listener);

    const payload = { kind: "data" as const, data: new Uint8Array([1, 2]) };
    ptyBus.emit("wt-1", payload);

    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener).toHaveBeenCalledWith(payload);
    unsub();
  });

  it("emit does not deliver to unsubscribed listeners", () => {
    const listener = vi.fn();
    const unsub = ptyBus.subscribe("wt-1", listener);
    unsub();

    ptyBus.emit("wt-1", { kind: "data" as const });
    expect(listener).not.toHaveBeenCalled();
  });

  it("emit does not cross worktree channels", () => {
    const listener1 = vi.fn();
    const listener2 = vi.fn();
    const unsub1 = ptyBus.subscribe("wt-1", listener1);
    const unsub2 = ptyBus.subscribe("wt-2", listener2);

    ptyBus.emit("wt-1", { kind: "exit" as const, code: 0 });

    expect(listener1).toHaveBeenCalledTimes(1);
    expect(listener2).not.toHaveBeenCalled();

    unsub1();
    unsub2();
  });

  it("supports multiple listeners on the same channel", () => {
    const l1 = vi.fn();
    const l2 = vi.fn();
    const unsub1 = ptyBus.subscribe("wt-1", l1);
    const unsub2 = ptyBus.subscribe("wt-1", l2);

    ptyBus.emit("wt-1", { kind: "scrollback" as const });

    expect(l1).toHaveBeenCalledTimes(1);
    expect(l2).toHaveBeenCalledTimes(1);

    unsub1();
    unsub2();
  });
});
