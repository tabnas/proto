// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package tabnasproto

// parity_test.go — cross-runtime conformance, driven by the shared
// `test/spec/*.tsv` fixtures at the repo root (see ../test/AGENTS.md), the
// same convention @tabnas/parser and @tabnas/abnf use.
//
// ts/test/parity.test.ts discovers and runs the SAME files, so the two
// implementations cannot drift without one of them going red.

import (
	"bufio"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

type specRow struct {
	file     string
	lineNo   int
	input    string
	expected string
	opts     string
}

func specDir() string { return filepath.Join("..", "test", "spec") }

// specUnescape decodes the escape set used in non-JSON columns. Kept
// byte-identical to the TS loader so both runtimes feed the parser the exact
// same source text.
func specUnescape(s string) string {
	if !strings.Contains(s, `\`) {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\\' && i+1 < len(s) {
			switch s[i+1] {
			case 'n':
				b.WriteByte('\n')
				i++
				continue
			case 'r':
				b.WriteByte('\r')
				i++
				continue
			case 't':
				b.WriteByte('\t')
				i++
				continue
			case '\\':
				b.WriteByte('\\')
				i++
				continue
			}
		}
		b.WriteByte(c)
	}
	return b.String()
}

func loadSpec(t *testing.T, path string) []specRow {
	t.Helper()
	f, err := os.Open(path)
	if err != nil {
		t.Fatalf("open %s: %v", path, err)
	}
	defer f.Close()

	var rows []specRow
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	lineNo := 0
	for scanner.Scan() {
		lineNo++
		if lineNo == 1 {
			continue // header naming the columns
		}
		// Strip the CR of a CRLF line: the TS loader splits on /\r?\n/ and
		// drops it, so keeping it here would feed the runtimes different bytes.
		line := strings.TrimSuffix(scanner.Text(), "\r")
		// A comment line starts with '#' and has no tab; a data row always
		// has at least one (input + expected), so '#'-leading sources still
		// work.
		if line == "" || (strings.HasPrefix(line, "#") && !strings.Contains(line, "\t")) {
			continue
		}
		cols := strings.Split(line, "\t")
		if len(cols) < 2 {
			t.Fatalf("%s:%d: expected at least 2 tab-separated columns", path, lineNo)
		}
		row := specRow{
			file:     filepath.Base(path),
			lineNo:   lineNo,
			input:    specUnescape(cols[0]),
			expected: cols[1],
		}
		if 3 <= len(cols) {
			row.opts = cols[2]
		}
		rows = append(rows, row)
	}
	if err := scanner.Err(); err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	if len(rows) == 0 {
		t.Fatalf("%s: no cases", path)
	}
	return rows
}

// specLabel is a truncated single-line rendering of the input, so a failure
// names its case readably.
func specLabel(s string) string {
	one := strings.Join(strings.Fields(s), " ")
	if 60 < len(one) {
		return one[:57] + "..."
	}
	return one
}

// specOpts decodes the `opts` column into ProtoOptions. The column uses the
// TS spelling (`{"version":"proto3","reconcile":false}`).
func specOpts(t *testing.T, row specRow) *ProtoOptions {
	t.Helper()
	if strings.TrimSpace(row.opts) == "" {
		return nil
	}
	var raw struct {
		Version   ProtoVersion `json:"version"`
		Reconcile *bool        `json:"reconcile"`
	}
	if err := json.Unmarshal([]byte(row.opts), &raw); err != nil {
		t.Fatalf("%s:%d: bad opts JSON %q: %v", row.file, row.lineNo, row.opts, err)
	}
	return &ProtoOptions{Version: raw.Version, Reconcile: raw.Reconcile}
}

func runSpecFile(t *testing.T, path string) {
	for _, row := range loadSpec(t, path) {
		t.Run(specLabel(row.input), func(t *testing.T) {
			got, err := Parse(row.input, specOpts(t, row))

			if strings.HasPrefix(row.expected, "ERROR") {
				want := strings.TrimPrefix(strings.TrimPrefix(row.expected, "ERROR"), ":")
				if err == nil {
					t.Fatalf("%s:%d: expected error, got %v", row.file, row.lineNo, got)
				}
				if want != "" && !strings.Contains(err.Error(), want) {
					t.Fatalf("%s:%d: expected error %q, got %q", row.file, row.lineNo, want, err.Error())
				}
				return
			}
			if err != nil {
				t.Fatalf("%s:%d: unexpected parse error: %v", row.file, row.lineNo, err)
			}

			// Round-trip through JSON so absent fields and field order do not
			// affect the structural comparison.
			gotJSON, err := json.Marshal(got)
			if err != nil {
				t.Fatalf("marshal: %v", err)
			}
			var gotVal, want any
			if err := json.Unmarshal(gotJSON, &gotVal); err != nil {
				t.Fatalf("unmarshal %s: %v", gotJSON, err)
			}
			if err := json.Unmarshal([]byte(row.expected), &want); err != nil {
				t.Fatalf("%s:%d: bad expected JSON %q: %v", row.file, row.lineNo, row.expected, err)
			}
			if !reflect.DeepEqual(gotVal, want) {
				t.Errorf("%s:%d:\n  got  %s\n  want %s", row.file, row.lineNo, gotJSON, row.expected)
			}
		})
	}
}

// TestSpec auto-discovers every fixture: adding a .tsv runs it in both
// runtimes without touching either runner.
func TestSpec(t *testing.T) {
	files, err := filepath.Glob(filepath.Join(specDir(), "*.tsv"))
	if err != nil {
		t.Fatalf("glob spec dir: %v", err)
	}
	if len(files) == 0 {
		t.Fatalf("no spec files under %s", specDir())
	}
	for _, path := range files {
		t.Run(filepath.Base(path), func(t *testing.T) { runSpecFile(t, path) })
	}
}
