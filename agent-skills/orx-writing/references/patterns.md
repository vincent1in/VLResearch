# Writing patterns: examples and sources

Contents: 1. Exposition and structure · 2. Claims and evidence ·
3. Word choice · 4. Equations, figures and notation · 5. Replies in chat ·
6. Sources

Each entry gives the pattern, an example, and why it weakens the text. These
are editing prompts, not rules: keep an instance that serves the argument.

## 1. Exposition and structure

**Counted symmetry in titles.** "One spectral filter, two notations"; "Two
ICs answer two questions." Titles engineered to be memorable read like
slides. Use a descriptive title.

**Corrective antithesis.** "The answer is about a joint spectrum, not the
condition number of one matrix." Useful once; recurring, it becomes a tic.
State the positive claim and mention the contrast only where a reader would
otherwise confuse the two.

**Staged misconception and reveal.** "Ridge regression is often introduced
as a numerical repair for collinearity. That description is incomplete." The
setup invents drama around an ordinary explanation. Give the fuller account
directly, with a citation if the narrower view is really common.

**Rebutting an objection nobody raised.** "That is not an omission"; "The
difference is not a contradiction." Give the mathematical reason instead.

**Grand compression claims.** "The argument in one sentence", repeated later
as "The book's argument fits in one sentence." Promotional when repeated.

**Slogan headings.** "Structure beats tuning." "The penalty matters." "The
criteria disagree." Replace with headings that name the content: "Effect of
the penalty on out-of-sample IC".

**Manufactured counts.** "Four facts stand out"; "adds two lessons"; "A
transformer adds three things to a head." Present the items; the reader can
count.

**Over-polished mirror sentences.** "Attention retrieves from the context,
and the MLP retrieves from the training corpus." The symmetry makes the claim
look cleaner and more categorical than its qualifications allow.

**Staging the exposition as questions.** "A nonlinear model raises an
additional question: which geometry should the data select?" then "Here the
question changes..." The prose keeps announcing its own agenda. Make the
statement.

**Headings that explain the explanation.** "What the bound says." "Why it
holds." "What the three conditions say." Used repeatedly, they shepherd the
reader. Fewer, plainer headings.

**Announced importance.** "This distinction matters even when two forecasts
have similar IC." Let the calculation that follows show it.

**Tour-guide transitions.** "Back to finance." "Forecast weights become
portfolio weights." Scripted segues give a lecture-video voice.

**Promises about other chapters.** "The next chapter measures...", "The next
chapter identifies conditions..." They pile up alongside opening roadmaps and
closing takeaways. Keep one roadmap where the document needs it.

**Recap boxes.** Advice to compare local linear regression with smoothing
splines and report tuning parameters, repeated in a "Two Baselines" box right
after. Keep the better instance.

**Repeated caveats.** "It does not bound the error of estimating the
coefficients..." followed shortly by "It does not bound coefficient-estimation
error." Say it once, where it applies.

**Admonitions after a result.** "The short-window return examples add another
discipline..."; "Fourth, more computation is not always better." A specific
result turned into a schoolroom moral. Let the result stand.

**Mechanical uniformity.** Every paragraph opens with a topic sentence, lists
three items and closes with a takeaway; the same transition word recurs.
Vary structure with the content.

## 2. Claims and evidence

**Inflated significance.** "a pivotal advance", "a testament to", "a
transformative framework". Replace with what was shown and how large it is:
"raises out-of-sample IC from 0.021 to 0.027 (paired SE 0.002, 12 folds)".

**Vague attribution.** "Studies have shown", "experts agree", "it is widely
believed". Cite the specific, checked source and the claim it supports, or
qualify or remove the sentence.

**Unqualified results.** "X improves performance." Say on what data, against
what baseline, over how many runs or trials, with what uncertainty. A best
result picked from many attempts needs the attempt count.

**Hedge stacking.** "This may potentially suggest a possible improvement."
One accurate qualifier is enough; say what the uncertainty is.

**Decorative participles.** "..., highlighting the importance of
regularization"; "..., underscoring its potential for practitioners." The
clause restates the claim with added weight. Explain the causal or
mathematical connection, or end the sentence earlier.

## 3. Word choice

**Repeated metaphors.** "What the linear component buys"; "how much GLS
buys"; "the information purchased by smoothness"; "the zero atom prices
hidden signal". One conversational device applied to unrelated results.
Describe the rate or decomposition literally.

**Agency for mathematical objects.** Ridge "decides"; an eigendirection
"prefers infinite shrinkage"; the optimizer "wants". Describe what the
estimator does under which conditions.

**Consultancy vocabulary.** "An operational playbook"; "freeze the
geometry"; "spectrum-first benign-overfitting audit". Name the procedure.

**Moralized precision.** "Honest constants", "honest validation", "use
calibrated language". Name the statistical property: "validation on folds
that postdate training", "constants that hold for finite n".

**Theatrical scope disclaimers.** "Not a classification of every
conceivable infinite-width construction." The limit is real; "every
conceivable" inflates it. "This covers constructions A and B only."

**Stock filler.** "In today's rapidly evolving landscape"; "plays a crucial
role"; "a rich tapestry"; closing "challenges and future prospects"
paragraphs that name no specific problem. Delete, or replace with the
concrete problem.

**Word lists are weak evidence.** Words such as "delve", "intricate" or
"underscore" became more frequent in papers after 2023, but that is a
population-level trend. One occurrence says nothing; change a word only when
a plainer one is more precise.

## 4. Equations, figures and notation

**An equation where a sentence does.** "The final score is
$s = \frac{1}{K}\sum_{k=1}^{K} s_k$", when $s$, $s_k$ and $K$ never appear
again. Write "the final score is the mean of the five fold scores".

**An equation where a figure does.** The bias-variance decomposition is
displayed, then a paragraph describes how each term moves as $\lambda$
grows. Plot squared bias, variance and their sum against $\lambda$ with the
minimum marked, and state the decomposition only if later steps use it.

**A figure and an equation together.** To explain ridge shrinkage, plot the
factor $d_j^2/(d_j^2 + \lambda)$ against $d_j^2$, the squared singular
values of $X$, for three values of $\lambda$. Label each curve with its
$\lambda$ and give the formula in the caption. The plot shows that
directions with small $d_j$ shrink most, the formula gives the exact amount,
and both use the same symbols.

**A derivation without words.** Four displays in a row, each rearranging
the last. Give each display one move and a clause saying what it did:
"expand the square"; "the cross term has mean zero because $\varepsilon$ is
independent of $x$"; "$\mathbb{E}[\varepsilon^2] = \sigma^2$ by assumption
(A2)".

**Case collisions.** "$k$ indexes folds" and later "$K$ is the number of
features". Write "$k = 1, \dots, K$ indexes the folds" and use $p$ for the
number of features. Likewise, with $T$ periods, write a transpose as
$X^\top$, not $X^T$.

**One symbol, two meanings.** $\lambda$ is the ridge penalty in one section
and an eigenvalue of $X^\top X$ in another, and the two meet in the
shrinkage factor. Write the eigenvalues as $d_j^2$ so the factor reads
$d_j^2/(d_j^2 + \lambda)$. Clashes common in finance: $r$ for a return and
for a correlation, $t$ for a period and for a $t$-statistic, $p$ for the
number of features and for a $p$-value.

**A silent change between steps.** "Center the columns of $X$. Then
$X^\top X / n$ is the covariance of the features." $X$ now names two
matrices. Write "let $\tilde{X}$ be $X$ with each column centered; then
$\tilde{X}^\top \tilde{X} / n$ is the empirical covariance of the
features".

**Names from code.** The text calls the penalty $\lambda$ and the figure's
axis says `alpha`. Label the axis $\lambda$ and say once that scikit-learn
calls it `alpha`.

**A symbol back after a gap.** $\kappa$ is defined in Section 2 and used in
Section 5 with no reminder. Write "the condition number $\kappa$" where it
returns.

## 5. Replies in chat

- Open with the answer, not with praise for the question or a restatement
  of it.
- Do not end with a recap of what was just said or a menu of offers. One
  real next step, if there is one.
- Use bullets and bold for content that is scanned or compared, not for
  every reply. Two short paragraphs often beat six bullets.
- Do not narrate effort ("I carefully reviewed..."); report what was found.
- When you are unsure, say what you checked and what you did not, once.
- Answer a conceptual question in words, with a figure or worked numbers
  where they help. Add notation only when the user needs the exact form,
  and define each symbol in the reply.

## 6. Sources

- William Zinsser, *On Writing Well* (30th anniversary ed., Harper
  Perennial, 2006): clutter, simplicity and the plain style.
- D. E. Knuth, T. Larrabee and P. M. Roberts, *Mathematical Writing*, MAA
  Notes 14 (Mathematical Association of America, 1989). Rule 14: "Don't use
  the same notation for two different things. Conversely, use consistent
  notation for the same thing when it appears in several places." Rule 11:
  "Try to state things twice, in complementary ways, especially when giving
  a definition", which is the case for pairing a figure with an equation.
- P. R. Halmos, "How to write mathematics", *L'Enseignement Mathématique*
  16 (1970), section "Think about the alphabet": settle notation before
  writing rather than mid-sentence, keep it consistent, and avoid clashes.
- Wikipedia, "Wikipedia:Signs of AI writing" (WikiProject AI Cleanup):
  inflated importance, trailing present participles, promotional language,
  vague attribution. The guide treats the signs as a field guide for
  editors, warns against automated detectors, and advises looking for
  patterns rather than single words.
- D. Kobak et al., "Delving into LLM-assisted writing in biomedical
  publications through excess vocabulary", *Science Advances* (2025): excess
  style words in PubMed abstracts after 2023.
- W. Liang et al., "Mapping the increasing use of LLMs in scientific
  papers" (2024): population-level estimates of LLM-modified text by field.

These studies describe trends across large corpora. None of them can
establish who wrote a particular passage, and neither can this list.
