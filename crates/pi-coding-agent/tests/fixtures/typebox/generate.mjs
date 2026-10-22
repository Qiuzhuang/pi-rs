// Regenerates expected.json from the real typebox package:
//   NODE_PATH=<dir>/node_modules node generate.mjs > expected.json   (run in this directory)
import { Type } from "typebox";
import { Value } from "typebox/value";
import { Compile } from "typebox/compile";
import { run } from "./cases.mjs";
console.log(JSON.stringify(run(Type, Value, Compile), null, 1));
