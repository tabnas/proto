// convert-goldens.go — turn the raw extraction of protobuf's parser_unittest.cc
// into the three corpus lane files the TS and Go conformance runners read.
//
// This tool is OURS; its OUTPUT is third-party and is never committed. It is
// copied into a throwaway module by scripts/fetch-protobuf-corpus.sh and run
// with GOWORK=off, so it never appears in this repo's go.mod or the shared
// go.work. It deliberately has no `package main` build tag issues: it is not
// part of any module in this repo (there is no go.mod beside it).
//
// The `expectedText` of a valid case is a FileDescriptorProto in protobuf TEXT
// format. It is decoded with the canonical Go protobuf runtime and re-emitted
// as canonical protojson, so the goldens are upstream's own values mechanically
// transcoded — never hand-written by us.

package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/encoding/prototext"
	"google.golang.org/protobuf/types/descriptorpb"
)

type rawCase struct {
	Name         string  `json:"name"`
	Helper       string  `json:"helper"`
	Input        string  `json:"input"`
	ExpectedText *string `json:"expectedText"`
	Error        *string `json:"error"`
	Note         string  `json:"note"`
}

type raw struct {
	Valid      []rawCase        `json:"valid"`
	Invalid    []rawCase        `json:"invalid"`
	AcceptOnly []rawCase        `json:"acceptOnly"`
	Excluded   []map[string]any `json:"excluded"`
	ByHelper   map[string]int   `json:"byHelper"`
}

type outCase struct {
	Name     string          `json:"name"`
	Helper   string          `json:"helper"`
	Input    string          `json:"input"`
	Expected json.RawMessage `json:"expected,omitempty"`
	Error    string          `json:"error,omitempty"`
	Note     string          `json:"note,omitempty"`
}

func write(dir, file string, v any) {
	b, err := json.MarshalIndent(v, "", " ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(dir, file), append(b, '\n'), 0644); err != nil {
		panic(err)
	}
}

func str(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

func main() {
	b, err := os.ReadFile(os.Args[1])
	if err != nil {
		panic(err)
	}
	outDir := os.Args[2]
	var r raw
	if err := json.Unmarshal(b, &r); err != nil {
		panic(err)
	}

	valid := []outCase{}
	for _, c := range r.Valid {
		if c.ExpectedText == nil {
			panic("valid lane case without a descriptor golden: " + c.Name)
		}
		var fdp descriptorpb.FileDescriptorProto
		if err := prototext.Unmarshal([]byte(*c.ExpectedText), &fdp); err != nil {
			panic(fmt.Sprintf("%s: prototext: %v", c.Name, err))
		}
		j, err := protojson.Marshal(&fdp)
		if err != nil {
			panic(err)
		}
		// protojson emits deliberately unstable whitespace; renormalise so the
		// corpus is byte-stable across runs.
		var norm any
		if err := json.Unmarshal(j, &norm); err != nil {
			panic(err)
		}
		j2, _ := json.Marshal(norm)
		valid = append(valid, outCase{Name: c.Name, Helper: c.Helper,
			Input: c.Input, Expected: j2})
	}

	invalid := []outCase{}
	for _, c := range r.Invalid {
		invalid = append(invalid, outCase{Name: c.Name, Helper: c.Helper,
			Input: c.Input, Error: str(c.Error)})
	}

	acceptOnly := []outCase{}
	for _, c := range r.AcceptOnly {
		acceptOnly = append(acceptOnly, outCase{Name: c.Name, Helper: c.Helper,
			Input: c.Input, Error: str(c.Error), Note: c.Note})
	}

	write(outDir, "valid.json", valid)
	write(outDir, "invalid.json", invalid)
	write(outDir, "accept-only.json", acceptOnly)
	write(outDir, "excluded.json", r.Excluded)

	fmt.Printf("convert: valid=%d invalid=%d accept-only=%d excluded=%d\n",
		len(valid), len(invalid), len(acceptOnly), len(r.Excluded))
}
