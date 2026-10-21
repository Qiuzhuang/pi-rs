// Compact TypeBox (`typebox` 1.x / `@sinclair/typebox`) builder for pi extensions running in pi-jsrt.
//
// Builders produce the same JSON Schema objects as TypeBox: the enumerable properties are the
// schema, and the TypeBox markers (`~kind`, `~optional`, `~readonly`) are hidden, non-enumerable
// properties. `Static<T>` and the other type-level helpers are erased by TypeScript stripping.

function hide(target, key, value) {
	Object.defineProperty(target, key, { configurable: true, writable: true, enumerable: false, value });
	return target;
}

function copyHidden(from, to) {
	for (const key of ["~kind", "~optional", "~readonly", "~immutable", "~unsafe"]) {
		if (from && Object.prototype.hasOwnProperty.call(from, key)) hide(to, key, from[key]);
	}
	return to;
}

function create(kind, schema, options) {
	const result = { ...schema, ...(options ?? {}) };
	return hide(result, "~kind", kind);
}

function clone(schema) {
	if (Array.isArray(schema)) return schema.map(clone);
	if (schema === null || typeof schema !== "object") return schema;
	const out = {};
	for (const key of Object.keys(schema)) out[key] = clone(schema[key]);
	return copyHidden(schema, out);
}

export function IsOptional(schema) {
	return !!schema && typeof schema === "object" && Object.prototype.hasOwnProperty.call(schema, "~optional");
}

export function IsReadonly(schema) {
	return !!schema && typeof schema === "object" && Object.prototype.hasOwnProperty.call(schema, "~readonly");
}

export function IsKind(schema, kind) {
	return !!schema && typeof schema === "object" && schema["~kind"] === kind;
}

export function IsSchema(value) {
	return !!value && typeof value === "object" && !Array.isArray(value);
}

function requiredKeys(properties) {
	return Object.keys(properties).filter((key) => !IsOptional(properties[key]));
}

export function String(options) {
	return create("String", { type: "string" }, options);
}
export function Number(options) {
	return create("Number", { type: "number" }, options);
}
export function Integer(options) {
	return create("Integer", { type: "integer" }, options);
}
export function Boolean(options) {
	return create("Boolean", { type: "boolean" }, options);
}
export function Null(options) {
	return create("Null", { type: "null" }, options);
}
export function Undefined(options) {
	return create("Undefined", { type: "undefined" }, options);
}
export function Void(options) {
	return create("Void", { type: "void" }, options);
}
export function BigInt(options) {
	return create("BigInt", { type: "bigint" }, options);
}
export function Symbol(options) {
	return create("Symbol", { type: "symbol" }, options);
}
export function Any(options) {
	return create("Any", {}, options);
}
export function Unknown(options) {
	return create("Unknown", {}, options);
}
export function Never(options) {
	return create("Never", { not: {} }, options);
}
export function Literal(value, options) {
	const type =
		typeof value === "bigint"
			? "bigint"
			: typeof value === "boolean"
				? "boolean"
				: typeof value === "number"
					? "number"
					: typeof value === "string"
						? "string"
						: (() => {
								throw new Error("Invalid Literal value");
							})();
	return create("Literal", { type, const: value }, options);
}
export function Object_(properties = {}, options = {}) {
	const required = requiredKeys(properties);
	return create("Object", { type: "object", ...(required.length > 0 ? { required } : {}), properties }, options);
}
export function Array_(items, options) {
	return create("Array", { type: "array", items }, options);
}
export function Tuple(types = [], options = {}) {
	return create("Tuple", { type: "array", additionalItems: false, items: types, minItems: types.length }, options);
}
export function Union(anyOf = [], options = {}) {
	return create("Union", { anyOf }, options);
}
export function Intersect(allOf = [], options = {}) {
	return create("Intersect", { allOf }, options);
}
export function Enum(value, options) {
	let values = value;
	if (!Array.isArray(value) && value && typeof value === "object") {
		// TypeScript enum objects: numeric enums have reverse mappings.
		values = Object.keys(value)
			.filter((key) => isNaN(globalThis.Number(key)))
			.map((key) => value[key]);
	}
	return create("Enum", { enum: values }, options);
}
export function Record(key, value, options = {}) {
	let pattern = "^.*$";
	if (IsKind(key, "Integer")) pattern = "^-?(?:0|[1-9][0-9]*)$";
	else if (IsKind(key, "Number")) pattern = "^-?(?:0|[1-9][0-9]*)(?:.[0-9]+)?$";
	else if (key && typeof key.pattern === "string") pattern = key.pattern;
	else if (IsKind(key, "Union") || IsKind(key, "Literal") || IsKind(key, "Enum")) {
		const keys = IsKind(key, "Literal")
			? [key.const]
			: IsKind(key, "Enum")
				? key.enum
				: key.anyOf.filter((k) => IsKind(k, "Literal")).map((k) => k.const);
		const properties = {};
		for (const k of keys) properties[k] = value;
		return Object_(properties, options);
	}
	return create("Record", { type: "object", patternProperties: { [pattern]: value } }, options);
}
export function Ref(ref, options) {
	return create("Ref", { $ref: typeof ref === "string" ? ref : ref?.$id }, options);
}
export function Unsafe(schema = {}) {
	return hide({ ...schema }, "~unsafe", null);
}
export function Optional(schema, flag = true) {
	const result = clone(schema);
	if (flag === false) {
		delete result["~optional"];
		return result;
	}
	return hide(result, "~optional", "Optional");
}
export function Readonly(schema, flag = true) {
	const result = clone(schema);
	if (flag === false) {
		delete result["~readonly"];
		return result;
	}
	return hide(result, "~readonly", "Readonly");
}
export function ReadonlyOptional(schema) {
	return Readonly(Optional(schema));
}
export function Immutable(schema) {
	return hide(clone(schema), "~immutable", true);
}
export function Partial(schema, options = {}) {
	if (!IsSchema(schema) || !schema.properties) return clone(schema);
	const properties = {};
	for (const [key, value] of Object.entries(schema.properties)) properties[key] = Optional(value);
	const { properties: _p, required: _r, type: _t, ...rest } = schema;
	return Object_(properties, { ...rest, ...options });
}
export function Required(schema, options = {}) {
	if (!IsSchema(schema) || !schema.properties) return clone(schema);
	const properties = {};
	for (const [key, value] of Object.entries(schema.properties)) properties[key] = Optional(value, false);
	const { properties: _p, required: _r, type: _t, ...rest } = schema;
	return Object_(properties, { ...rest, ...options });
}
function keyList(keys) {
	if (Array.isArray(keys)) return keys;
	if (IsKind(keys, "Literal")) return [keys.const];
	if (IsKind(keys, "Union")) return keys.anyOf.map((k) => k.const);
	if (IsKind(keys, "Enum")) return keys.enum;
	return [];
}
export function Pick(schema, keys, options = {}) {
	const wanted = new Set(keyList(keys));
	const properties = {};
	for (const [key, value] of Object.entries(schema.properties ?? {})) if (wanted.has(key)) properties[key] = value;
	return Object_(properties, options);
}
export function Omit(schema, keys, options = {}) {
	const unwanted = new Set(keyList(keys));
	const properties = {};
	for (const [key, value] of Object.entries(schema.properties ?? {})) if (!unwanted.has(key)) properties[key] = value;
	return Object_(properties, options);
}
export function KeyOf(schema, options = {}) {
	return Union(
		Object.keys(schema.properties ?? {}).map((key) => Literal(key)),
		options,
	);
}
export function Composite(schemas, options = {}) {
	const properties = {};
	for (const schema of schemas) Object.assign(properties, schema.properties ?? {});
	return Object_(properties, options);
}
export function Evaluate(schema) {
	return schema;
}
export function Function(parameters, returns, options) {
	return create("Function", { type: "function", parameters, returns }, options);
}
export function Promise_(item, options) {
	return create("Promise", { type: "promise", item }, options);
}
export function Date_(options) {
	return create("Date", { type: "Date" }, options);
}
export function Uint8Array_(options) {
	return create("Uint8Array", { type: "Uint8Array" }, options);
}
export function RegExp_(pattern, options) {
	const source = typeof pattern === "string" ? pattern : pattern.source;
	return create("String", { type: "string", pattern: source }, options);
}
export function TemplateLiteral(parts, options) {
	return create("TemplateLiteral", { type: "string", pattern: "^.*$" }, options);
}
export function Refine(schema) {
	return schema;
}
export function Codec(schema) {
	const builder = {
		Decode: () => builder,
		Encode: () => schema,
	};
	return builder;
}
export function Cyclic(defs, ref, options) {
	return create("Cyclic", { $defs: defs, $ref: ref }, options);
}
export function Script() {
	throw new Error("Type.Script is not supported in pi-rs extensions");
}
export function Not(schema, options) {
	return create("Not", { not: schema }, options);
}
export function Exclude(union, excluded, options) {
	const remove = new Set(JSON.stringify(excluded));
	return Union(
		(union.anyOf ?? []).filter((s) => !remove.has(JSON.stringify(s))),
		options,
	);
}
export function Extract(union, extracted, options) {
	return clone(extracted);
}

const builders = {
	Any,
	Array: Array_,
	BigInt,
	Boolean,
	Codec,
	Composite,
	Cyclic,
	Date: Date_,
	Enum,
	Evaluate,
	Exclude,
	Extract,
	Function,
	Immutable,
	Integer,
	Intersect,
	KeyOf,
	Literal,
	Never,
	Not,
	Null,
	Number,
	Object: Object_,
	Omit,
	Optional,
	Partial,
	Pick,
	Promise: Promise_,
	Readonly,
	ReadonlyOptional,
	Record,
	Ref,
	Refine,
	RegExp: RegExp_,
	Required,
	Script,
	String,
	Symbol,
	TemplateLiteral,
	Tuple,
	Uint8Array: Uint8Array_,
	Undefined,
	Union,
	Unknown,
	Unsafe,
	Void,
	IsOptional,
	IsReadonly,
	IsSchema,
};

/** The `Type` namespace. Unknown builders produce a permissive schema instead of failing. */
export const Type = new Proxy(builders, {
	get(target, prop) {
		if (typeof prop !== "string" || prop in target) return target[prop];
		if (prop === "then") return undefined;
		return (...args) => {
			const options = args.find((a) => a && typeof a === "object" && !Array.isArray(a) && !("~kind" in a));
			return create(prop, {}, options);
		};
	},
});

export { Object_ as Object, Array_ as Array, Promise_ as Promise, Date_ as Date, RegExp_ as RegExp };

export const Kind = "~kind";
export const OptionalKind = "~optional";
export const ReadonlyKind = "~readonly";

// `typebox/value` and `typebox/compile` subset: structural checks against the schema.
function check(schema, value) {
	if (!schema || typeof schema !== "object") return true;
	if (schema.anyOf) return schema.anyOf.some((s) => check(s, value));
	if (schema.allOf) return schema.allOf.every((s) => check(s, value));
	if (schema.not) return !check(schema.not, value);
	if ("const" in schema) return value === schema.const;
	if (schema.enum) return schema.enum.includes(value);
	switch (schema.type) {
		case "string":
			return typeof value === "string";
		case "number":
			return typeof value === "number" && globalThis.Number.isFinite(value);
		case "integer":
			return typeof value === "number" && globalThis.Number.isInteger(value);
		case "boolean":
			return typeof value === "boolean";
		case "null":
			return value === null;
		case "undefined":
			return value === undefined;
		case "array":
			if (!globalThis.Array.isArray(value)) return false;
			if (globalThis.Array.isArray(schema.items)) return schema.items.every((s, i) => check(s, value[i]));
			return schema.items ? value.every((v) => check(schema.items, v)) : true;
		case "object": {
			if (value === null || typeof value !== "object" || globalThis.Array.isArray(value)) return false;
			for (const key of schema.required ?? []) if (!(key in value)) return false;
			for (const [key, s] of Object.entries(schema.properties ?? {})) {
				if (value[key] !== undefined && !check(s, value[key])) return false;
			}
			if (schema.patternProperties) {
				const patterns = Object.entries(schema.patternProperties).map(([p, s]) => [new RegExp(p), s]);
				for (const [key, v] of Object.entries(value)) {
					for (const [re, s] of patterns) if (re.test(key) && !check(s, v)) return false;
				}
			}
			return true;
		}
		default:
			return true;
	}
}

export const Value = {
	Check: (schema, value) => check(schema, value),
	Clone: (value) => structuredClone(value),
	Parse: (schema, value) => {
		if (!check(schema, value)) throw new Error("Value does not match schema");
		return value;
	},
	Errors: (schema, value) => (check(schema, value) ? [] : [{ message: "Value does not match schema", value }]),
	Default: (schema, value) => value,
	Convert: (schema, value) => value,
	Clean: (schema, value) => value,
	Create: (schema) => {
		if ("default" in schema) return structuredClone(schema.default);
		switch (schema.type) {
			case "string":
				return "";
			case "number":
			case "integer":
				return 0;
			case "boolean":
				return false;
			case "array":
				return [];
			case "object": {
				const out = {};
				for (const key of schema.required ?? []) out[key] = Value.Create(schema.properties[key]);
				return out;
			}
			default:
				return undefined;
		}
	},
};

export function Compile(schema) {
	return {
		Check: (value) => check(schema, value),
		Errors: (value) => Value.Errors(schema, value),
		Parse: (value) => Value.Parse(schema, value),
	};
}

export const TypeCompiler = { Compile };

export default Type;
