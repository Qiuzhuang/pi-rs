export const cases = {
  stringFormat: (T) => T.String({ format: "uri" }),
  optionalUnionDefault: (T) => T.Optional(T.Union([T.String(), T.Number()], { default: 1 })),
  arrayOfObjects: (T) => T.Array(T.Object({ id: T.Integer({ minimum: 1 }) }, { additionalProperties: false })),
  index: (T) => T.Index(T.Object({ a: T.String(), b: T.Number() }), ["a"]),
  mapped: (T) => T.Mapped ? T.Partial(T.Pick(T.Object({ a: T.String(), b: T.Number() }), ["a"])) : null,
  string: (T) => T.String(),
  stringOpts: (T) => T.String({ description: "d", minLength: 1 }),
  number: (T) => T.Number({ minimum: 0, description: "n" }),
  integer: (T) => T.Integer(),
  boolean: (T) => T.Boolean({ default: false }),
  null: (T) => T.Null(),
  any: (T) => T.Any({ description: "x" }),
  unknown: (T) => T.Unknown(),
  never: (T) => T.Never(),
  undef: (T) => T.Undefined(),
  void: (T) => T.Void(),
  bigint: (T) => T.BigInt(),
  litS: (T) => T.Literal("a"),
  litN: (T) => T.Literal(1, { description: "one" }),
  litB: (T) => T.Literal(true),
  object: (T) => T.Object({ a: T.String(), b: T.Optional(T.Number()) }),
  objectOpts: (T) => T.Object({ a: T.String() }, { additionalProperties: false, description: "o" }),
  objectEmpty: (T) => T.Object({}),
  objectAllOpt: (T) => T.Object({ a: T.Optional(T.String({ description: "a" })) }),
  array: (T) => T.Array(T.String(), { minItems: 1, description: "arr" }),
  tuple: (T) => T.Tuple([T.String(), T.Number()]),
  tupleEmpty: (T) => T.Tuple([]),
  union: (T) => T.Union([T.Literal("a"), T.Literal("b")], { description: "u" }),
  intersect: (T) => T.Intersect([T.Object({ a: T.String() }), T.Object({ b: T.Number() })]),
  enumArr: (T) => T.Enum(["a", "b"]),
  enumObj: (T) => T.Enum({ A: "a", B: "b" }),
  enumNum: (T) => { const E = { A: 0, B: 1, 0: "A", 1: "B" }; return T.Enum(E); },
  recordS: (T) => T.Record(T.String(), T.Number()),
  recordN: (T) => T.Record(T.Number(), T.Number()),
  recordI: (T) => T.Record(T.Integer(), T.Number()),
  recordU: (T) => T.Record(T.Union([T.Literal("x"), T.Literal("y")]), T.Number()),
  recordL: (T) => T.Record(T.Literal("x"), T.Boolean()),
  ref: (T) => T.Ref("Foo"),
  unsafe: (T) => T.Unsafe({ type: "string", enum: ["a"] }),
  optional: (T) => T.Optional(T.String()),
  optionalOff: (T) => T.Optional(T.Optional(T.String()), false),
  readonly: (T) => T.Readonly(T.String()),
  readonlyOptional: (T) => T.ReadonlyOptional(T.String()),
  partial: (T) => T.Partial(T.Object({ a: T.String(), b: T.Number() }, { description: "p" })),
  required: (T) => T.Required(T.Object({ a: T.Optional(T.String()), b: T.Number() })),
  pick: (T) => T.Pick(T.Object({ a: T.String(), b: T.Number(), c: T.Boolean() }), ["a", "c"]),
  pickU: (T) => T.Pick(T.Object({ a: T.String(), b: T.Number() }), T.Union([T.Literal("b")])),
  omit: (T) => T.Omit(T.Object({ a: T.String(), b: T.Optional(T.Number()) }), ["a"]),
  keyof: (T) => T.KeyOf(T.Object({ a: T.String(), b: T.Number() })),
  composite: (T) => T.Composite([T.Object({ a: T.String() }), T.Object({ b: T.Optional(T.Number()) })]),
  regexp: (T) => T.RegExp(/abc/),
  regexpS: (T) => T.RegExp("^a"),
  templ: (T) => T.TemplateLiteral([T.Literal("a"), T.String()]),
  not: (T) => T.Not(T.String()),
  exclude: (T) => T.Exclude(T.Union([T.Literal("a"), T.Literal("b")]), T.Literal("a")),
  extract: (T) => T.Extract(T.Union([T.Literal("a"), T.Literal("b")]), T.Literal("a")),
  date: (T) => T.Date(),
  u8: (T) => T.Uint8Array(),
  func: (T) => T.Function([T.String()], T.Number()),
  promise: (T) => T.Promise(T.String()),
  symbol: (T) => T.Symbol(),
  immutable: (T) => T.Immutable(T.Object({ a: T.String() })),
  nested: (T) => T.Object({
    path: T.String({ description: "Path" }),
    edits: T.Array(T.Object({ oldText: T.String(), newText: T.String() }), { description: "E" }),
    mode: T.Optional(T.Union([T.Literal("a"), T.Literal("b")])),
    meta: T.Optional(T.Record(T.String(), T.Any())),
  }),
  stringEnumLike: (T) => T.Unsafe({ type: "string", enum: ["a", "b"], description: "x" }),
  literalOptsOrder: (T) => T.Literal("a", { description: "d", default: "a" }),
  cyclic: (T) => T.Cyclic({ A: T.Object({ a: T.Optional(T.Ref("A")) }) }, "A"),
  evaluate: (T) => T.Evaluate(T.Intersect([T.Object({ a: T.String() }), T.Object({ b: T.Number() })])),
};

/** Describe a schema: enumerable JSON plus hidden (non-enumerable) TypeBox markers, in key order. */
export function describe(v) {
  if (v === null || typeof v !== "object") return v;
  if (Array.isArray(v)) return v.map(describe);
  const out = {};
  for (const k of Object.getOwnPropertyNames(v)) {
    const d = Object.getOwnPropertyDescriptor(v, k);
    out[d.enumerable ? k : `(hidden)${k}`] = describe(v[k]);
  }
  return out;
}

/** Run every case: `{ name: { json, full } | { error: true } }`, plus Value / Compile checks. */
export function run(T, Value, Compile) {
  const res = {};
  for (const [name, f] of Object.entries(cases)) {
    try {
      const s = f(T);
      res[name] = { json: JSON.stringify(s), full: JSON.stringify(describe(s)) };
    } catch {
      res[name] = { error: true };
    }
  }
  const schema = T.Object({ a: T.String(), b: T.Optional(T.Integer({ minimum: 0 })) });
  const values = [{ a: "x" }, { a: "x", b: 2 }, { a: "x", b: -1 }, { a: 1 }, {}, { a: "x", b: 1.5 }];
  res.valueCheck = values.map((v) => Value.Check(schema, v));
  res.compileCheck = values.map((v) => Compile(schema).Check(v));
  res.valueErrors = [...Value.Errors(schema, { a: 1 })].length;
  res.valueCreate = JSON.stringify(Value.Create(schema));
  res.valueDefault = JSON.stringify(Value.Default(T.Object({ n: T.Number({ default: 3 }) }), {}));
  return res;
}
