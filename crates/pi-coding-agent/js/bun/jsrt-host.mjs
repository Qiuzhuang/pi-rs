// `jsrt:host` for the bun backend: `ref(value)` returns a `{"$jsHandle": id}` marker for the
// value (primitives pass through), matching pi-jsrt's builtin of the same name.
const H = globalThis.__piHandles;

export function ref(value) {
	return H.mark(value);
}

// Bun transpiles TypeScript natively; this is a no-op kept for API compatibility.
export function transpile(code, _filename) {
	return String(code);
}
