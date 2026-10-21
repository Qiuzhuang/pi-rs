// `@earendil-works/pi-agent-core` for extensions running in pi-jsrt. Extensions use its types
// (erased by TypeScript stripping); the agent itself runs in Rust, so runtime classes are stubs.

import { Type } from "typebox";

export { Type };

function unavailable(name) {
	return class {
		constructor() {
			throw new Error(`${name} is not available to extensions in pi-rs`);
		}
	};
}

export const Agent = unavailable("Agent");

export function agentLoop() {
	throw new Error("agentLoop is not available to extensions in pi-rs");
}

export function agentLoopContinue() {
	throw new Error("agentLoopContinue is not available to extensions in pi-rs");
}

export default { Type, Agent };
