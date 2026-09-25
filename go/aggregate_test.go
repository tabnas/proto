/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Go port of the "aggregate option values" suite in ts/test/proto.test.ts.
// test/spec/aggregate.tsv holds the rows; this pins the API paths.

package tabnasproto

import (
	"math"
	"regexp"
	"strings"
	"testing"
	"time"

	tabnas "github.com/tabnas/parser/go"
)

// protoc records the text between the braces, comments turned to the
// spaces and newlines that keep each token where it was (protoc 36.2's
// parser gives exactly this string for this source).
const (
	aggregateSample     = "message M {\n  option (f) = {\n    a: 1 // one\n    b { c: \"x\" /* two */ }\n  };\n}\n"
	aggregateSampleText = "\n    a: 1 \n    b { c: \"x\"           }\n  "
)

func TestAggregateRecordsTheTextBetweenTheBraces(t *testing.T) {
	fdp := mustParse(t, aggregateSample, nil)
	if got := fdp.MessageType[0].Options["(f)"]; got != aggregateSampleText {
		t.Errorf("aggregate: got %q, want %q", got, aggregateSampleText)
	}
}

// The walk has only the CST, so the text has to be on the CST: Proto puts
// it on the aggregate's `constant` node as "aggregate", and leaves "src"
// the tokens run together, as on every node. The C library takes this
// path: an engine built once, then ToDescriptor(j.Parse(src)).
func TestAggregateReadsTheSameThroughToDescriptor(t *testing.T) {
	cst, err := aggregateEngine(t).Parse(aggregateSample)
	if err != nil {
		t.Fatal(err)
	}
	fdp, err := ToDescriptor(cst, nil)
	if err != nil {
		t.Fatal(err)
	}
	if got := fdp.MessageType[0].Options["(f)"]; got != aggregateSampleText {
		t.Errorf("ToDescriptor: got %q, want %q", got, aggregateSampleText)
	}
	var find func(n any) map[string]any
	find = func(n any) map[string]any {
		m, _ := n.(map[string]any)
		if m == nil {
			return nil
		}
		if nrule(m) == "constant" {
			return m
		}
		for _, k := range nkids(m) {
			if found := find(k); found != nil {
				return found
			}
		}
		return nil
	}
	node := find(cst)
	if node == nil {
		t.Fatal("no constant node in the CST")
	}
	if got := nsrc(node); got != `{a:1b{c:"x"}}` {
		t.Errorf("src: got %q", got)
	}
	if got := node["aggregate"]; got != aggregateSampleText {
		t.Errorf("aggregate on the node: got %q, want %q", got, aggregateSampleText)
	}

	// A value inside the braces is a `constant`, a single string included.
	if got := entryKids(cst, `c:"x"`); strings.Join(got, " ") != "constant" {
		t.Errorf(`entry c:"x": got rules %q, want [constant]`, got)
	}
}

func TestAggregateTakesAdjacentStringLiterals(t *testing.T) {
	const src = `message M { option (f) = { a: "x" "y" }; }`
	fdp := mustParse(t, src, nil)
	if got := fdp.MessageType[0].Options["(f)"]; got != ` a: "x" "y" ` {
		t.Errorf("adjacent strings: got %q", got)
	}

	// Two or more literals are the entry's `strLit` children, one each.
	cst, err := aggregateEngine(t).Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	if got := entryKids(cst, `a:"x""y"`); strings.Join(got, " ") != "strLit strLit" {
		t.Errorf(`entry a:"x""y": got rules %q, want [strLit strLit]`, got)
	}
}

// aggregateEngine is an engine built once with Proto installed, the way
// the C library builds one.
func aggregateEngine(t *testing.T) *tabnas.Tabnas {
	t.Helper()
	rh := 8192
	j := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &rh}})
	if err := Proto(j); err != nil {
		t.Fatal(err)
	}
	return j
}

// entryKids is the rules under the entry whose src is entry: the CST shape
// of one value inside the braces. Nil when the tree has no such entry.
func entryKids(n any, entry string) []string {
	m, _ := n.(map[string]any)
	if m == nil {
		return nil
	}
	if nrule(m) == "messageValueEntry" && nsrc(m) == entry {
		rules := []string{}
		for _, k := range nkids(m) {
			rules = append(rules, nrule(k))
		}
		return rules
	}
	for _, k := range nkids(m) {
		if found := entryKids(k, entry); found != nil {
			return found
		}
	}
	return nil
}

// Go port of the "text format inside an aggregate" and "string literals"
// suites in ts/test/proto.test.ts. The fixtures in test/spec/aggregate.tsv
// and test/spec/adjacent-strings.tsv hold the rows; these pin the CST
// shapes and the matcher's word list.

// ruleKids is the rules under the node carrying rule and src.
func ruleKids(n any, rule, src string) []string {
	m, _ := n.(map[string]any)
	if m == nil {
		return nil
	}
	if nrule(m) == rule && nsrc(m) == src {
		rules := []string{}
		for _, k := range nkids(m) {
			rules = append(rules, nrule(k))
		}
		return rules
	}
	for _, k := range nkids(m) {
		if found := ruleKids(k, rule, src); found != nil {
			return found
		}
	}
	return nil
}

func TestAggregateReadsListsAndAngleBracketsAsNodes(t *testing.T) {
	cst, err := aggregateEngine(t).Parse(`option (f) = { a: [1, "x" "y"] b < c: 1 > };`)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct{ rule, src, want string }{
		{"messageValueEntry", `a:[1,"x""y"]`, "listValue"},
		{"listValue", `[1,"x""y"]`, "constant constant"},
		{"messageValueEntry", "b<c:1>", "angleValue"},
		{"angleValue", "<c:1>", "messageValueEntry"},
	} {
		if got := strings.Join(ruleKids(cst, c.rule, c.src), " "); got != c.want {
			t.Errorf("%s %s: got %q, want %q", c.rule, c.src, got, c.want)
		}
	}
}

func TestAggregateReadsKeywordsAndBracketedNamesAsIdentifiersInsideOnly(t *testing.T) {
	const src = "option (f) = { message: optional [ x . y ]: 1 };\nmessage M { optional int32 a = 1; }"
	cst, err := aggregateEngine(t).Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	// The bracketed name is one word, its spaces left out as every node's
	// src leaves them out.
	if got := strings.Join(ruleKids(cst, "messageValueEntry", "[x.y]:1"), " "); got != "constant" {
		t.Errorf("entry [x.y]:1: got %q", got)
	}
	fdp, err := ToDescriptor(cst, nil)
	if err != nil {
		t.Fatal(err)
	}
	if got := fdp.Options["(f)"]; got != " message: optional [ x . y ]: 1 " {
		t.Errorf("aggregate: got %q", got)
	}
	if fdp.MessageType[0].Name != "M" {
		t.Errorf("message after the aggregate: got %q", fdp.MessageType[0].Name)
	}
	// Outside an aggregate a keyword is a keyword, as it was in 0.5.0.
	if _, err := Parse("option (f) = max;", nil); err == nil {
		t.Error("a keyword as a plain option value: want the refusal 0.5.0 gave")
	}
}

func TestAggregateKeywordsMatchTheGrammar(t *testing.T) {
	quoted := regexp.MustCompile(`"([A-Za-z_][A-Za-z0-9_]*)"`)
	words := map[string]bool{}
	for _, line := range strings.Split(GrammarText, "\n") {
		// A rule line ends at the first `;` outside a quoted literal.
		var body strings.Builder
		inQuote := false
		for _, c := range line {
			if c == '"' {
				inQuote = !inQuote
			} else if c == ';' && !inQuote {
				break
			}
			body.WriteRune(c)
		}
		for _, m := range quoted.FindAllStringSubmatch(body.String(), -1) {
			words[strings.ToLower(m[1])] = true
		}
	}
	delete(words, "export")
	delete(words, "local")
	if len(words) != len(aggregateKeywords) {
		t.Errorf("the grammar spells %d keywords, the matcher knows %d", len(words), len(aggregateKeywords))
	}
	for w := range words {
		if !aggregateKeywords[w] {
			t.Errorf("the matcher does not know the grammar's keyword %q", w)
		}
	}
}

func TestStringLiteralsOneAsWrittenAdjacentAsProtocReadsThem(t *testing.T) {
	// protoc decodes both. One literal is kept as written, escapes and all,
	// as 0.5.0 kept it; adjacent literals, which 0.5.0 refused, are recorded
	// as protoc records them: decoded and concatenated.
	fdp := mustParse(t, `option (f) = "\x41"; option (g) = "\x41" "";`+"\n"+`import "a\x41";`+"\n"+`import "a" "\x41";`, nil)
	if got := fdp.Options["(f)"]; got != `\x41` {
		t.Errorf("one literal: got %q", got)
	}
	if got := fdp.Options["(g)"]; got != "A" {
		t.Errorf("adjacent literals: got %q", got)
	}
	if got := strings.Join(fdp.Dependency, " "); got != `a\x41 aA` {
		t.Errorf("imports: got %q", got)
	}
}

// The answer to "is the lexer inside an aggregate" once came from walking up
// the rule stack, which every top-level definition and every aggregate entry
// deepens, so each keyword cost more than the one before it. Port of the
// "telling inside an aggregate from outside" suite in ts/test/proto.test.ts.
func TestInAggregateAsksOnlyTheRuleAtHand(t *testing.T) {
	brace := &tabnas.Token{Src: "{"}
	constant := &tabnas.Rule{Name: "constant", O0: brace}
	lex := &tabnas.Lex{Ctx: &tabnas.Context{}}
	// The parent is an aggregate's constant, but only the mark says so.
	unmarked := &tabnas.Rule{Name: "messageValueEntry", Parent: constant}
	if inAggregate(lex, unmarked) {
		t.Error("an unmarked rule: inAggregate walked up the rule stack")
	}
	marked := &tabnas.Rule{Name: "messageValueEntry", Parent: constant, K: map[string]any{insideKey: true}}
	if !inAggregate(lex, marked) {
		t.Error("a marked rule: want inside")
	}
	if !inAggregate(lex, constant) {
		t.Error("the aggregate's own constant: want inside")
	}
	peeking := &tabnas.Lex{Ctx: &tabnas.Context{T: []*tabnas.Token{brace}}}
	if !inAggregate(peeking, &tabnas.Rule{Name: "constant"}) {
		t.Error("a constant with the brace in the lookahead: want inside")
	}
	// markAggregate marks a rule an aggregate's constant pushed, and no other.
	markAggregate(unmarked, nil)
	if !inAggregate(lex, unmarked) {
		t.Error("markAggregate did not mark a rule the aggregate pushed")
	}
	plain := &tabnas.Rule{Name: "constant", O0: &tabnas.Token{Src: "1"}}
	other := &tabnas.Rule{Name: "constant$alt4", Parent: plain}
	markAggregate(other, nil)
	if inAggregate(lex, other) {
		t.Error("markAggregate marked a rule under a constant that is not an aggregate")
	}
}

// A keyword deep in angle brackets costs what any other word costs. On the
// walk this replaced, `message <` nested 2,000 deep took longer than
// `abcdefg <`, and the gap grew with the square of the depth. The comparison
// is between spellings of one shape, so it holds on any machine and whatever
// the engine's own curve.
func TestInAggregateCostsTheSameForAKeyword(t *testing.T) {
	j := aggregateEngine(t)
	best := func(src string) time.Duration {
		fastest := time.Duration(math.MaxInt64)
		for i := 0; i < 3; i++ {
			start := time.Now()
			if _, err := j.Parse(src); err != nil {
				t.Fatal(err)
			}
			fastest = min(fastest, time.Since(start))
		}
		return fastest
	}
	angle := func(word string) string {
		return "option (f) = {" + strings.Repeat(" "+word+" <", 2000) + strings.Repeat(" >", 2000) + " };"
	}
	keyword, plain := best(angle("message")), best(angle("abcdefg"))
	if keyword > plain*6/5+50*time.Millisecond {
		t.Errorf("%v with a keyword, against %v without", keyword, plain)
	}
}
