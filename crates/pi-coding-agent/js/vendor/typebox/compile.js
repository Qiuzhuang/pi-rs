// typebox 1.3.27 (typebox, typebox/value, typebox/compile), bundled by scripts/vendor-typebox.mjs.
// Do not edit; regenerate instead.
/*
TypeBox

The MIT License (MIT)

Copyright (c) 2017-2026 Haydn Paterson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
import{b as c,d as p,i as f,k as l,l as x,p as C,s as d,t as y,v as E,y as g,z as T}from"pi-rs:typebox/chunk-2";import{U as i,f as m}from"pi-rs:typebox/chunk-1";function u(){return"// @ts-ignore"}function a(){return""}function R(e){return[...e.UseUnevaluated()?['import { CheckContext } from "typebox/schema"']:[],'import { Hashing } from "typebox/system"','import { Format } from "typebox/format"','import { Guard } from "typebox/guard"']}function S(e){let{identifier:t}=e.External();return[a(),u(),`let ${t} = []`,a(),u(),`export function SetExternal(external) { ${t} = external.variables }`]}function P(e){return e.Functions().map(t=>[a(),u(),`${t};`].join(`
`))}function $(e){let t=e.UseUnevaluated()?`const context = new CheckContext({}, {}); return ${e.Entry()}`:`return ${e.Entry()}`;return[a(),u(),`export function Check(value) { ${t} }`]}function v(...e){let[t,r]=i.Match(e,{2:(h,k)=>[h,k],1:h=>[{},h]}),o=c(t,r),s=[...R(o),...S(o),...P(o),...$(o)].join(`
`);return{External:o.External(),Code:s}}var n=class{constructor(t,r){this.hasCodec=y(t,r),this.buildResult=c(t,r),this.evaluateResult=this.buildResult.Evaluate()}IsAccelerated(){return this.evaluateResult.IsAccelerated()}Context(){return this.buildResult.Context()}Type(){return this.buildResult.Schema()}Code(){return this.evaluateResult.Code()}Check(t){return this.evaluateResult.Check(t)}Parse(t){if(this.Check(t))return t;if(m.Get().correctiveParse)return T(this.Context(),this.Type(),t);throw new g(t,this.Errors(t))}Errors(t){return this.IsAccelerated()&&this.Check(t)?[]:p(this.Context(),this.Type(),t)}Clean(t){return f(this.Context(),this.Type(),t)}Convert(t){return l(this.Context(),this.Type(),t)}Create(){return E(this.Context(),this.Type())}Default(t){return x(this.Context(),this.Type(),t)}Decode(t){return this.hasCodec?C(this.Context(),this.Type(),t):this.Parse(t)}Encode(t){return this.hasCodec?d(this.Context(),this.Type(),t):this.Parse(t)}};function b(...e){let[t,r]=i.Match(e,{2:(o,s)=>[o,s],1:o=>[{},o]});return new n(t,r)}var A=b;export{v as Code,b as Compile,n as Validator,A as default};
