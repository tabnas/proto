// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// The rewind history proto's parsers keep: the engine's default, 64
// consumed tokens. The compiled grammar has no probe, the one builtin that
// rewinds, so no parse rewinds; the one reader of the history is
// recordAggregate, which reads the last token consumed (ctx.V1). Every
// document the window test reads parses alike at 64 and at 8,192, the bound
// proto kept through 0.7.0, a longer history only holding a window's text
// back to its oldest token. ts/test/history.test.ts and
// rs/tests/history_test.rs hold the TypeScript and Rust ports to the same.

import (
	"fmt"
	"regexp"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

func TestTheGrammarHasNoProbeAndTheParsersKeepTheDefaultHistory(t *testing.T) {
	if regexp.MustCompile(`@probe|"pd_`).Match(grammarSpec) {
		t.Fatal("the compiled grammar holds a probe")
	}
	for _, p := range windowParsers {
		j, err := p.build()
		if err != nil {
			t.Fatal(err)
		}
		if got := j.Config().RewindHistory; got != 64 {
			t.Fatalf("%s keeps a history of %d", p.name, got)
		}
	}
}

func TestEveryDocumentParsesAlikeAtTheDefaultHistoryAndAt8192(t *testing.T) {
	documents := windowDocuments(t)
	documents = append(documents, windowDocument{at: "the long document", text: longDocument()})
	for index, text := range longErrorDocuments() {
		documents = append(documents, windowDocument{at: fmt.Sprintf("long error document %d", index), text: text})
	}
	short := tabnas.Make()
	if err := Proto(short); err != nil {
		t.Fatal(err)
	}
	history := 8192
	long := tabnas.Make(tabnas.Options{Rewind: &tabnas.RewindOptions{History: &history}})
	if err := Proto(long); err != nil {
		t.Fatal(err)
	}
	for _, document := range documents {
		if got, want := outcome(short.Parse(document.text)), outcome(long.Parse(document.text)); got != want {
			t.Fatalf("%s:\nat 64    %.300s\nat 8,192 %.300s", document.at, got, want)
		}
	}
	t.Logf("%d documents alike at 64 and at 8,192", len(documents))
}
