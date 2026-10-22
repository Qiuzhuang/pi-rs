//! Port of `packages/tui/test/latex.test.ts` (`renderLatex`): every case of the TypeScript
//! file, grouped as there (one table per `describe` of `defineCases`, one test per `it`).

use pi_tui::latex::{RenderLatexOptions, render_latex};

fn inline(source: &str) -> Option<String> {
    render_latex(source, RenderLatexOptions { display: false })
}

fn display(source: &str) -> Option<String> {
    render_latex(source, RenderLatexOptions { display: true })
}

fn check_table(cases: &[(&str, &str)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(source, expected)| {
            let got = inline(source);
            (got.as_deref() != Some(*expected)).then(|| format!("renders {source:?}\n  got  {got:?}\n  want {expected:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "{} case(s) failed:\n{}", failures.len(), failures.join("\n"));
}

/// renderLatex > Jacobian conjecture session using dollar delimiters
#[test]
fn jacobian_conjecture_session_using_dollar_delimiters() {
    check_table(&[
        (r"\mathbb{C}^3 \to \mathbb{C}^3", "ℂ³ → ℂ³"),
        (r"\{3x+2y,\; 27x^2-4z-1,\; x(x-1)(x+1)\} \quad\Rightarrow\quad x \in \{0, \pm 1\},", "{3x+2y, 27x²-4z-1, x(x-1)(x+1)} ⇒ x ∈ {0, ± 1},"),
        (r"F_1 = -\frac{1}{4x^2}.", "F₁ = -1/(4x²)."),
        ("-2", "-2"),
        ("(0,0,-1/4)", "(0,0,-1/4)"),
        ("(1,-3/2,13/2)", "(1,-3/2,13/2)"),
        ("(1,1,1)", "(1,1,1)"),
        ("(2,1,0)", "(2,1,0)"),
        ("(-1/4, 0, 0)", "(-1/4, 0, 0)"),
        (r"\{(0,0,-1/4), (1,-3/2,13/2), (-1,3/2,13/2)\}", "{(0,0,-1/4), (1,-3/2,13/2), (-1,3/2,13/2)}"),
        ("(2,1,1)", "(2,1,1)"),
        ("(7/3,-2/5,11/7)", "(7/3,-2/5,11/7)"),
        (r"\{y - p(x),\; q(x)\}", "{y - p(x), q(x)}"),
        (r"\deg q = 3", "deg q = 3"),
        (r"[\mathbb{C}(x,y,z):\mathbb{C}(F_1,F_2,F_3)] = 3", "[ℂ(x,y,z):ℂ(F₁,F₂,F₃)] = 3"),
        ("u = 1+xy", "u = 1+xy"),
        ("G = u^2 z + y^2(4+3xy)", "G = u² z + y²(4+3xy)"),
        ("F_1 = uG", "F₁ = uG"),
        ("F_2 = y + 3xG", "F₂ = y + 3xG"),
        ("x=0", "x = 0"),
        ("F_2 = F_3 = 0", "F₂ = F₃ = 0"),
        ("xy = -3/2", "xy = -3/2"),
        ("x^2 z = 13/2", "x² z = 13/2"),
        (r"\mathbb{C}^*", "ℂ^*"),
        (r"s \mapsto (s,\, -\tfrac{3}{2s},\, \tfrac{13}{2s^2})", "s ↦ (s, -3/(2s), 13/(2s²))"),
        ("X", "X"),
        (r"p_\pm", "p_±"),
        ("F(-x,-y,z) = (F_1, -F_2, -F_3)", "F(-x,-y,z) = (F₁, -F₂, -F₃)"),
        ("p_0", "p₀"),
        (r"s \to \infty", "s → ∞"),
        ("(0,0,0)", "(0,0,0)"),
        (r"\Rightarrow", "⇒"),
        (r"\ge 2", "≥ 2"),
        (r"\ge 3", "≥ 3"),
        ("1", "1"),
        (r"\mathrm{diag}(-1/2,1,1),\quad F_{\rm intrinsic}(\lambda)", "diag(-1/2,1,1), F_intrinsic(λ)"),
        ("4+3xy", "4+3xy"),
    ]);
}

/// renderLatex > satellite calculation session using bracket delimiters
#[test]
fn satellite_calculation_session_using_bracket_delimiters() {
    check_table(&[
        (r"E \approx \frac{0.1\ \text{lux}}{100\ \text{lm/W}} = 0.001\ \text{W/m}^2", "E ≈ (0.1 lux)/(100 lm/W) = 0.001 W/m²"),
        (r"\boxed{1\ \text{milliwatt per square metre}}", "[1 milliwatt per square metre]"),
        (r"5\ \text{km}^2 = 5{,}000{,}000\ \text{m}^2", "5 km² = 5,000,000 m²"),
        (r"P_{\text{light}} = 0.001 \times 5{,}000{,}000
= \boxed{5{,}000\ \text{W}}", "P_light = 0.001 × 5,000,000 = [5,000 W]"),
        (r"P_{\text{electric}} = 5\ \text{kW} \times 0.2
= \boxed{1\ \text{kW}}", "P_electric = 5 kW × 0.2 = [1 kW]"),
        (r"\pi(2.5\ \text{km})^2 = 19.6\ \text{km}^2", "π(2.5 km)² = 19.6 km²"),
        (r"0.001\ \text{W/m}^2 \times 19.6 \times 10^6\ \text{m}^2
\approx \boxed{20\ \text{kW optical}}", "0.001 W/m² × 19.6 × 10⁶ m² ≈ [20 kW optical]"),
        (r"1\ \text{kW} \times \frac{1}{3600}\ \text{hour}
= \boxed{0.28\ \text{Wh}}", "1 kW × 1/3600 hour = [0.28 Wh]"),
    ]);
}

/// renderLatex > Jacobian conjecture sessions using parenthesis and bracket delimiters
#[test]
fn jacobian_conjecture_sessions_using_parenthesis_and_bracket_delimiters() {
    check_table(&[
        (r"\det\!\left(\frac{\partial(F_1,F_2,F_3)}{\partial(x,y,z)}\right)=-2.", "det((∂(F₁,F₂,F₃))/(∂(x,y,z))) = -2."),
        (r"\begin{aligned}
F(0,0,-\tfrac14)&=(-\tfrac14,0,0),\\
F(1,-\tfrac32,\tfrac{13}2)&=(-\tfrac14,0,0),\\
F(-1,\tfrac32,\tfrac{13}2)&=(-\tfrac14,0,0).
\end{aligned}", r"F(0,0,-1/4) = (-1/4,0,0),
F(1,-3/2,13/2) = (-1/4,0,0),
F(-1,3/2,13/2) = (-1/4,0,0)."),
        ("F=(F_1,F_2,F_3)", "F = (F₁,F₂,F₃)"),
        ("F", "F"),
        ("3", "3"),
    ]);
}

/// renderLatex > Jacobian matrix session using dollar delimiters
#[test]
fn jacobian_matrix_session_using_dollar_delimiters() {
    check_table(&[
        (r"J = \begin{pmatrix}
\frac{\partial f_1}{\partial x} & \frac{\partial f_1}{\partial y} & \frac{\partial f_1}{\partial z} \\
\frac{\partial f_2}{\partial x} & \frac{\partial f_2}{\partial y} & \frac{\partial f_2}{\partial z} \\
\frac{\partial f_3}{\partial x} & \frac{\partial f_3}{\partial y} & \frac{\partial f_3}{\partial z}
\end{pmatrix}", r"J = ⎛ (∂ f₁)/(∂ x) │ (∂ f₁)/(∂ y) │ (∂ f₁)/(∂ z) ⎞
    ⎜ (∂ f₂)/(∂ x) │ (∂ f₂)/(∂ y) │ (∂ f₂)/(∂ z) ⎟
    ⎝ (∂ f₃)/(∂ x) │ (∂ f₃)/(∂ y) │ (∂ f₃)/(∂ z) ⎠"),
        (r"\begin{aligned}
f_1 &= (1+xy)^3 z + y^2(1+xy)(4+3xy) \\
f_2 &= y + 3x(1+xy)^2 z + 3xy^2(4+3xy) \\
f_3 &= 2x - 3x^2y - x^3z
\end{aligned}", r"f₁ = (1+xy)³ z + y²(1+xy)(4+3xy)
f₂ = y + 3x(1+xy)² z + 3xy²(4+3xy)
f₃ = 2x - 3x²y - x³z"),
        ("x, y, z", "x, y, z"),
        ("(x, y, z)", "(x, y, z)"),
        (r"(0,\; 0,\; -\tfrac14)", "(0, 0, -1/4)"),
        (r"(-\tfrac14,\; 0,\; 0)", "(-1/4, 0, 0)"),
        (r"(1,\; -\tfrac32,\; \tfrac{13}{2})", "(1, -3/2, 13/2)"),
        (r"(-1,\; \tfrac32,\; \tfrac{13}{2})", "(-1, 3/2, 13/2)"),
        (r"(-\frac14, 0, 0)", "(-1/4, 0, 0)"),
        (r"F: \mathbb{C}^3 \to \mathbb{C}^3", "F: ℂ³ → ℂ³"),
        (r"F(0,0,-\tfrac14) = F(1,-\tfrac32,\tfrac{13}{2}) = F(-1,\tfrac32,\tfrac{13}{2}) = (-\tfrac14, 0, 0)", "F(0,0,-1/4) = F(1,-3/2,13/2) = F(-1,3/2,13/2) = (-1/4, 0, 0)"),
        (r"\mathbb{C}^3", "ℂ³"),
        (r"\begin{aligned}
f_1 &= \frac{f_1^{\text{ut}}(u,t)}{x^2}, \quad
f_2 = \frac{f_2^{\text{ut}}(u,t)}{x}, \quad
f_3 = x\,(2 - 3u - t)
\end{aligned}", "f₁ = (f₁ᵘᵗ(u,t))/(x²), f₂ = (f₂ᵘᵗ(u,t))/x, f₃ = x (2 - 3u - t)"),
        (r"\det J_F", "det J_F"),
        (r"(-\tfrac14, 0, 0)", "(-1/4, 0, 0)"),
        ("u = xy", "u = xy"),
        ("t = x^2z", "t = x²z"),
        (r"x \neq 0", "x ≠ 0"),
        (r"f_1^{\text{ut}}, f_2^{\text{ut}}", "f₁ᵘᵗ, f₂ᵘᵗ"),
        ("u,t", "u,t"),
        ("x", "x"),
        ("x, x^2", "x, x²"),
        (r"\mathbb{C}^n \to \mathbb{C}^n", "ℂⁿ → ℂⁿ"),
        (r"n \geq 2", "n ≥ 2"),
        (r"\mathbb{P}^3", "ℙ³"),
    ]);
}

/// renderLatex > extended formulas from a renderer stress-test session
#[test]
fn extended_formulas_from_a_renderer_stress_test_session() {
    check_table(&[
        (r"e^{i\pi}+1=0", "e^(iπ)+1 = 0"),
        (r"\boxed{
\mathcal{Z}(\beta)
=
\int_{\mathcal M}
\exp\!\left(
-\beta\left[
\frac12 g^{ij}(x)\,\partial_i\phi\,\partial_j\phi
+V(\phi)
\right]\right)
\mathcal D\phi
}", "[Z(β) = ∫_M exp( -β[ 1/2 gⁱʲ(x) ∂ᵢϕ ∂ⱼϕ +V(ϕ) ]) Dϕ]"),
        (r"\begin{aligned}
\nabla_\mu T^{\mu\nu}
&=
\frac{1}{\sqrt{-g}}
\partial_\mu\!\left(\sqrt{-g}\,T^{\mu\nu}\right)
+\Gamma^\nu_{\mu\lambda}T^{\mu\lambda}
=0, \\[4pt]
R_{\mu\nu}-\frac12 Rg_{\mu\nu}+\Lambda g_{\mu\nu}
&=
\frac{8\pi G}{c^4}T_{\mu\nu}.
\end{aligned}", r"∇_μ T^(μν) = 1/(√(-g)) ∂_μ(√(-g) T^(μν)) +Γ^ν_(μλ)T^(μλ) = 0,
R_(μν)-1/2 Rg_(μν)+Λ g_(μν) = (8π G)/(c⁴)T_(μν)."),
        (r"f(z)
=
\frac{1}{2\pi i}
\oint_{\gamma}
\frac{f(\zeta)}{\zeta-z}\,d\zeta,
\qquad
\det\!\begin{pmatrix}
\lambda-a & -b & 0\\
-c & \lambda-d & -e\\
0 & -f & \lambda-g
\end{pmatrix}
=0.", r"f(z) = 1/(2π i) ∮_γ (f(ζ))/(ζ-z) dζ, det⎛ λ-a │ -b  │ 0   ⎞ = 0.
                                        ⎜ -c  │ λ-d │ -e  ⎟
                                        ⎝ 0   │ -f  │ λ-g ⎠"),
        (r"\Psi(x,t)=
\sum_{n=1}^{\infty}
\underbrace{
c_n
\sqrt{\frac{2}{L}}
\sin\!\left(\frac{n\pi x}{L}\right)
}_{\text{spatial eigenmode}}
\exp\!\left(-\frac{i\hbar n^2\pi^2}{2mL^2}t\right),
\qquad
|\Psi(x,t)|^2
=
\begin{cases}
\Psi^\ast\Psi, & 0<x<L,\\
0, & \text{otherwise}.
\end{cases}", r"                                                                                                 ⎧ Ψ^∗Ψ if 0 < x < L,
Ψ(x,t) = ∑ₙ₌₁^∞ cₙ √(2/L) sin((nπ x)/L)_(spatial eigenmode) exp(-(iℏ n²π²)/(2mL²)t), |Ψ(x,t)|² = ⎨
                                                                                                 ⎩ 0    otherwise."),
        (r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}", "x = (-b±√(b²-4ac))/(2a)"),
        (r"\int_0^\infty e^{-x^2}\,dx=\frac{\sqrt{\pi}}{2}", "∫₀^∞ e^(-x²) dx = (√π)/2"),
        (r"e^{i\theta}=\cos\theta+i\sin\theta", "e^(iθ) = cos θ+i sin θ"),
        (r"\sum_{n=1}^{\infty}\frac{1}{n^2}=\frac{\pi^2}{6}", "∑ₙ₌₁^∞1/(n²) = π²/6"),
        (r"\lim_{x\to 0}\frac{\sin x}{x}=1", "lim[x→0] (sin x)/x = 1"),
        (r"\lim_{n\to\infty}
\left(1+\frac{1}{n}\right)^n=e", "lim[n→∞] (1+1/n)ⁿ = e"),
        (r"\int_0^1 \frac{x^2}{1+x^3}\,dx
=\frac{1}{3}\ln 2", "∫₀¹ x²/(1+x³) dx = 1/3 ln 2"),
        (r"\sum_{k=1}^{n}\frac{k}{k+1}
=n+1-H_{n+1}", "∑ₖ₌₁ⁿk/(k+1) = n+1-Hₙ₊₁"),
        (r"\frac{
  \displaystyle \frac{x^2+1}{x-1}
  -
  \displaystyle \frac{2x}{x+1}
}{
  \displaystyle \frac{x}{x^2-1}
}", "((x²+1)/(x-1) - 2x/(x+1))/(x/(x²-1))"),
        (r"\lim_{x\to 0}
\frac{
  \displaystyle \frac{\sin x}{x}-1
}{
  \displaystyle \frac{e^x-1}{x}-1
}
=0", "lim[x→0] ((sin x)/x-1)/((eˣ-1)/x-1) = 0"),
        (r"\frac{
  1+\displaystyle\frac{1}{1+\frac{1}{x}}
}{
  1-\displaystyle\frac{1}{1-\frac{1}{x}}
}", "(1+1/(1+1/x))/(1-1/(1-1/x))"),
        (r"\sum_{n=1}^{\infty}
\frac{
  \displaystyle \frac{1}{n}-\frac{1}{n+1}
}{
  \displaystyle 1+\frac{1}{n^2}
}", "∑ₙ₌₁^∞ (1/n-1/(n+1))/(1+1/(n²))"),
    ]);
}

/// renderLatex > renders common symbols, roots, sums, and integrals
#[test]
fn renders_common_symbols_roots_sums_and_integrals() {
    assert_eq!(inline(r"\sum_{i=0}^n \alpha_i + \int_0^\infty e^{-x^2}\,dx = \sqrt{\pi}").as_deref(), Some("∑ᵢ₌₀ⁿ αᵢ + ∫₀^∞ e^(-x²) dx = √π"));
}

/// renderLatex > renders common accents and binomial notation
#[test]
fn renders_common_accents_and_binomial_notation() {
    assert_eq!(inline(r"\binom{n}{k}+\vec{x}+\hat{y}+\overline{AB}").as_deref(), Some("(n choose k)+x⃗+ŷ+overline(AB)"));
}

/// renderLatex > renders extended symbols and negated relations
#[test]
fn renders_extended_symbols_and_negated_relations() {
    assert_eq!(inline(r"\epsilon+\varepsilon+\varsigma+\varkappa+\oplus+\otimes+\therefore+\because").as_deref(), Some("ϵ+ε+ς+ϰ+⊕+⊗+∴+∵"));
    assert_eq!(inline(r"A\not\subseteq B,\quad x\not\in X").as_deref(), Some("A ⊈ B, x ∉ X"));
}

/// renderLatex > renders relational algebra join operators
#[test]
fn renders_relational_algebra_join_operators() {
    assert_eq!(inline(r"R\bowtie S,\quad R\Join S").as_deref(), Some("R ⋈ S, R ⋈ S"));
    assert_eq!(inline(r"R\ltimes S,\quad R\rtimes S").as_deref(), Some("R ⋉ S, R ⋊ S"));
    assert_eq!(inline(r"R\leftouterjoin S,\quad R\rightouterjoin S,\quad R\fullouterjoin S").as_deref(), Some("R ⟕ S, R ⟖ S, R ⟗ S"));
}

/// renderLatex > renders delimiter commands and invisible delimiters
#[test]
fn renders_delimiter_commands_and_invisible_delimiters() {
    assert_eq!(inline(r"\lvert{x}\rvert+\lVert{v}\rVert+\left.\frac{dy}{dx}\right|_{x=0}").as_deref(), Some("|x|+‖v‖+dy/(dx)|ₓ₌₀"));
    assert_eq!(inline(r"\left\lbrace x \middle| x>0 \right\rbrace").as_deref(), Some("{ x | x > 0 }"));
}

/// renderLatex > renders named, modular, overlaid, and underlaid operators
#[test]
fn renders_named_modular_overlaid_and_underlaid_operators() {
    assert_eq!(inline(r"\operatorname*{arg\,max}_{x\in X} f(x)").as_deref(), Some("arg max[x∈X] f(x)"));
    assert_eq!(inline(r"a\bmod n,\quad a\equiv b\pmod n").as_deref(), Some("a mod n, a ≡ b (mod n)"));
    assert_eq!(inline(r"\overset{!}{=}+\underset{n}{x}+\stackrel{def}{=}").as_deref(), Some("=^!+xₙ+=ᵈᵉᶠ"));
}

/// renderLatex > renders indexed roots and additional accents and wrappers
#[test]
fn renders_indexed_roots_and_additional_accents_and_wrappers() {
    assert_eq!(inline(r"\sqrt[2]{x}+\sqrt[3]{x}+\sqrt[4]{x}+\sqrt[n]{x}+\sqrt[k]{x+1}").as_deref(), Some("√x+∛x+∜x+ⁿ√x+ᵏ√(x+1)"));
    assert_eq!(inline(r"\acute{x}+\grave{y}+\widehat{xyz}+\overrightarrow{AB}").as_deref(), Some("x́+ỳ+widehat(xyz)+overrightarrow(AB)"));
    assert_eq!(inline(r"\textnormal{hello}+\mbox{world}+\boldsymbol{x}+{\rm roman}+{\bf bold}+{\it italic}+{\sf sans}+{\tt mono}+{\cal calligraphic}+{\sl slanted}").as_deref(), Some("hello+world+x+roman+bold+italic+sans+mono+calligraphic+slanted"));
}

/// renderLatex > renders additional display environments
#[test]
fn renders_additional_display_environments() {
    assert_eq!(inline(r"\begin{equation}\begin{split}a&=b\\&=c\end{split}\end{equation}").as_deref(), Some(r"a = b
= c"));
    assert_eq!(inline(r"\begin{alignedat}{2}a&=b&\quad c&=d\\e&=f&g&=h\end{alignedat}").as_deref(), Some(r"a = b c = d
e = f g = h"));
}

/// renderLatex > uses natural case conditions and aligns matrix columns
#[test]
fn uses_natural_case_conditions_and_aligns_matrix_columns() {
    assert_eq!(inline(r"f(x)=\begin{cases}a & x<0 \\ b & \text{if }x=0 \\ c & \text{otherwise}\end{cases}").as_deref(), Some(r"       ⎧ a if x < 0
f(x) = ⎨ b if x = 0
       ⎩ c otherwise"));
    assert_eq!(inline(r"\begin{pmatrix}1&200\\3000&4\end{pmatrix}").as_deref(), Some(r"⎛ 1    │ 200 ⎞
⎝ 3000 │ 4   ⎠"));
}

/// renderLatex > composes matrices with fractions and adjacent matrices
#[test]
fn composes_matrices_with_fractions_and_adjacent_matrices() {
    assert_eq!(display(r"R\left(\frac{\pi}{4}\right)
=
\begin{pmatrix}
\frac{\sqrt{2}}{2} & -\frac{\sqrt{2}}{2}\\
\frac{\sqrt{2}}{2} & \frac{\sqrt{2}}{2}
\end{pmatrix}.").as_deref(), Some(r"   π
R( ─ ) = ⎛ (√2)/2 │ -(√2)/2 ⎞
   4     ⎝ (√2)/2 │ (√2)/2  ⎠."));
    assert_eq!(display(r"\mathbf w
=
R\left(\frac{\pi}{4}\right)
\begin{pmatrix}1\\0\end{pmatrix}
=
\begin{pmatrix}\frac{\sqrt{2}}{2}\\\frac{\sqrt{2}}{2}\end{pmatrix}.").as_deref(), Some(r"       π
w = R( ─ ) ⎛ 1 ⎞ = ⎛ (√2)/2 ⎞
       4   ⎝ 0 ⎠   ⎝ (√2)/2 ⎠."));
    assert_eq!(display(r"A\mathbf e_1=\begin{pmatrix}\pi\\0\end{pmatrix},\qquad A\mathbf e_2=\begin{pmatrix}0\\\frac{1}{\pi}\end{pmatrix}.").as_deref(), Some(r"Ae₁ = ⎛ π ⎞, Ae₂ = ⎛ 0   ⎞
      ⎝ 0 ⎠        ⎝ 1/π ⎠."));
    assert_eq!(display(r"\sum_{i=0}^n x_i=\begin{pmatrix}a&b\\c&d\end{pmatrix}.").as_deref(), Some(r" n
 ∑  xᵢ = ⎛ a │ b ⎞
i=0      ⎝ c │ d ⎠."));
}

/// renderLatex > normalizes relation, multiplication, and named-operator spacing
#[test]
fn normalizes_relation_multiplication_and_named_operator_spacing() {
    assert_eq!(inline("x=y").as_deref(), Some("x = y"));
    assert_eq!(inline("x =y").as_deref(), Some("x = y"));
    assert_eq!(inline(r"x=
y").as_deref(), Some("x = y"));
    assert_eq!(inline(r"x
=
y").as_deref(), Some("x = y"));
    assert_eq!(inline("x_{i=0}").as_deref(), Some("xᵢ₌₀"));
    assert_eq!(inline(r"x\neq0").as_deref(), Some("x ≠ 0"));
    assert_eq!(inline(r"A\to B").as_deref(), Some("A → B"));
    assert_eq!(inline(r"\pi\cdot\frac{1}{\pi}").as_deref(), Some("π · 1/π"));
    assert_eq!(inline(r"\sin\theta").as_deref(), Some("sin θ"));
    assert_eq!(inline(r"\sin^2 x").as_deref(), Some("sin² x"));
    assert_eq!(inline(r"-\sin\theta").as_deref(), Some("-sin θ"));
    assert_eq!(inline(r"i\sin\theta").as_deref(), Some("i sin θ"));
    assert_eq!(inline(r"\det(A)").as_deref(), Some("det(A)"));
}

/// renderLatex > treats a backslash followed by a line ending as control space
#[test]
fn treats_a_backslash_followed_by_a_line_ending_as_control_space() {
    assert_eq!(display(r"\boxed{
(1,1,1),\ (1,1,2),\ (1,2,5),\ (1,5,13),\ (2,5,29),\
(1,13,34),\ (1,34,89)
}.").as_deref(), Some("[(1,1,1), (1,1,2), (1,2,5), (1,5,13), (2,5,29), (1,13,34), (1,34,89)]."));
    assert_eq!(inline("a\\\r\nb").as_deref(), Some("a b"));
}

/// renderLatex > stacks operator limits in display mode
#[test]
fn stacks_operator_limits_in_display_mode() {
    assert_eq!(display(r"\sum_{i=0}^n x_i").as_deref(), Some(r" n
 ∑  xᵢ
i=0"));
    assert_eq!(display(r"\min_{x\in X} f(x)").as_deref(), Some(r"min f(x)
x∈X"));
    assert_eq!(display(r"\operatorname*{arg\,max}_{x\in X} f(x)").as_deref(), Some(r"arg max f(x)
  x∈X"));
    assert_eq!(display(r"\int\nolimits_0^1 f(x)\,dx").as_deref(), Some("∫₀¹ f(x) dx"));
    assert_eq!(display(r"\int\limits_0^1 f(x)\,dx").as_deref(), Some(r"1
∫ f(x) dx
0"));
}

/// renderLatex > centers even case rows around a middle brace
#[test]
fn centers_even_case_rows_around_a_middle_brace() {
    assert_eq!(inline(r"f(x) = \begin{cases} x^{2} & x \geq 0 \\ -x & x < 0 \end{cases}").as_deref(), Some(r"       ⎧ x² if x ≥ 0
f(x) = ⎨
       ⎩ -x if x < 0"));
}

/// renderLatex > stacks fractions in display mode
#[test]
fn stacks_fractions_in_display_mode() {
    assert_eq!(display(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}").as_deref(), Some(r"    -b±√(b²-4ac)
x = ────────────
         2a"));
    assert_eq!(display(r"\frac{x^2+1}{x-1}").as_deref(), Some(r"x²+1
────
x-1"));
    assert_eq!(display(r"\frac{1}
{2}").as_deref(), Some(r"1
─
2"));
}

/// renderLatex > keeps nested display fractions linear
#[test]
fn keeps_nested_display_fractions_linear() {
    assert_eq!(display(r"\frac{\frac{x^2+1}{x-1}-\frac{2x}{x+1}}{\frac{x}{x^2-1}}").as_deref(), Some(r"(x²+1)/(x-1)-2x/(x+1)
─────────────────────
      x/(x²-1)"));
    assert_eq!(display(r"\lim_{x\to 0}\frac{\frac{\sin x}{x}-1}{\frac{e^x-1}{x}-1}=0").as_deref(), Some(r"     (sin x)/x-1
lim  ─────────── = 0
x→0  (eˣ-1)/x-1"));
    assert_eq!(display(r"\frac{1+\frac{1}{1+\frac{1}{x}}}{1-\frac{1}{1-\frac{1}{x}}}").as_deref(), Some(r"1+1/(1+1/x)
───────────
1-1/(1-1/x)"));
}

/// renderLatex > lays out unsupported and nested scripts while keeping script fractions linear
#[test]
fn lays_out_unsupported_and_nested_scripts_while_keeping_script_fractions_linear() {
    assert_eq!(display(r"\partial_tU_2(t,0)=Aj_*(1-t)^{-A-1}.\qquad x^{n^2}+x_{i_j}").as_deref(), Some(r"                            2
                    -A-1   n
∂ₜU₂(t,0) = Aj (1-t)    . x  +x
              *                i
                                j"));
    assert_eq!(display(r"e^{\frac{1}{2}}+\tfrac{1}{2}").as_deref(), Some("e^(1/2)+1/2"));
}

/// renderLatex > returns undefined for unsupported commands
#[test]
fn returns_undefined_for_unsupported_commands() {
    assert_eq!(inline(r"x + \unknown{y}").as_deref(), None);
}

/// renderLatex > returns undefined for malformed groups and environments
#[test]
fn returns_undefined_for_malformed_groups_and_environments() {
    assert_eq!(inline(r"\frac{1}{x").as_deref(), None);
    assert_eq!(inline("x}").as_deref(), None);
    assert_eq!(inline(r"\begin{matrix}1 & 2").as_deref(), None);
    assert_eq!(inline(r"x\").as_deref(), None);
}
