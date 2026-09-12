const started = Date.now();

function elapsed(): string {
  const seconds = Math.floor((Date.now() - started) / 1000);
  const minutes = String(Math.floor(seconds / 60)).padStart(2, "0");
  return `[${minutes}:${String(seconds % 60).padStart(2, "0")}]`;
}

/** One line per state transition: elapsed time, step label, state, detail. */
export function log(label: string, state: string, detail = ""): void {
  console.log(`${elapsed()} ${label.padEnd(26)} ${state.padEnd(10)} ${detail}`.trimEnd());
}
