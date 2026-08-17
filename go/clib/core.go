// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// core.go — the library's behaviour, in plain Go.
//
// tabnas-clib-template: v1 (stamped by admin tasks/adopt-clib.sh
// from tasks/clib-template/; edit the template and re-stamp, not this
// file — admin's verify gate fails on a stale stamp).
//
// libtabnasproto is the proto format parser as a C shared library, one of
// the per-format clibs sharing the uniform ABI decided by ADR-12: the
// same five symbols in every library, the format fixed at build time by
// which library you load. The cgo layer in tabnas_c.go is a thin shim
// over this file: it converts (pointer, length) pairs to Go strings and
// Go strings to malloc'd C strings, and does nothing else. Go does not
// support cgo in _test.go files, so anything living beside `import "C"`
// cannot be unit-tested at all; keeping the logic here is what makes
// the contract testable (core_test.go).
package main

import (
	"encoding/json"
	"strings"
	"sync"

	plug "github.com/tabnas/proto/go"
)

const (
	templateVersion = "v1"
	libName         = "libtabnasproto"
	formatName      = "proto"
	valueOut        = true
)

// One ready-to-parse engine for this format. Engines are not safe for
// concurrent Parse, and an FFI caller is under no obligation to
// serialise — CPython, for one, releases the GIL for the duration of a
// ctypes call — so each instance carries a mutex.
type parseFn func(string) (any, error)

type instance struct {
	mu    sync.Mutex
	parse parseFn
}

var (
	reg    sync.RWMutex
	nextID int64
	loaded = map[int64]*instance{}
)

// newParser builds one engine with the proto grammar installed
// NATIVELY — in-process, not via a serialized spec. That is a
// correctness decision, not a convenience: lexing configuration is part
// of the accepted language, and format plugins keep format-specific
// behaviour as closures, which cannot cross a data boundary at all.
// The body is the per-repo column of admin tasks/clib-rollout.tsv.
func newParser() (parseFn, error) {
	return func(src string) (any, error) { return plug.Parse(src, nil) }, nil
}

// reply marshals a result document. Marshalling cannot fail for the
// shapes built here, and there is nowhere to report it if it did, so a
// failure degrades to a fixed error document rather than something the
// caller cannot decode.
func reply(v map[string]any) string {
	out, err := json.Marshal(v)
	if err != nil {
		return `{"ok":false,"error":{"code":"internal",` +
			`"message":"result could not be encoded"}}`
	}
	return string(out)
}

func failDoc(code, message string) string {
	return reply(map[string]any{
		"ok":    false,
		"error": map[string]any{"code": code, "message": message},
	})
}

func firstLine(s string) string {
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return s[:i]
	}
	return s
}

func versionDoc() string {
	return reply(map[string]any{
		"ok": true, "lib": libName, "format": formatName,
		"template": templateVersion,
	})
}

// errPayload keeps a structured diagnostic when the engine provides one
// (TabnasError marshals to a JSON object) without this package needing
// to import the engine: marshal the error itself, and fall back to its
// first line when that yields nothing object-shaped.
func errPayload(err error) any {
	if b, jerr := json.Marshal(err); jerr == nil && len(b) > 2 && b[0] == '{' {
		return json.RawMessage(b)
	}
	return map[string]any{"message": firstLine(err.Error())}
}

// loadGrammar builds a parser instance and returns a handle to it.
//
// optsJSON is RESERVED: the uniform ABI (ADR-12) gives every format
// clib the same signature, and options are the obvious future use of
// the grammar argument a fixed-format library otherwise would not
// need. Until a version of this template defines them, anything but
// empty/{} is refused loudly — silently ignoring options would let a
// caller believe a configuration took effect when it did not.
func loadGrammar(optsJSON string) string {
	if optsJSON != "" {
		var o map[string]any
		if err := json.Unmarshal([]byte(optsJSON), &o); err != nil {
			return failDoc("usage", "options must be a JSON object")
		}
		if len(o) > 0 {
			return failDoc("usage",
				libName+" accepts no options yet; pass NULL (or {})")
		}
	}
	p, err := safeNew()
	if err != nil {
		return failDoc("grammar", firstLine(err.Error()))
	}
	reg.Lock()
	nextID++
	id := nextID
	loaded[id] = &instance{parse: p}
	reg.Unlock()
	return reply(map[string]any{"ok": true, "handle": id})
}

// safeNew contains construction panics. Parts of the compiler pipeline
// panic on inputs they cannot handle; panicking across a C ABI aborts
// the host process, which is never the right failure for a library.
func safeNew() (p parseFn, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = &panicErr{r}
		}
	}()
	return newParser()
}

type panicErr struct{ v any }

func (e *panicErr) Error() string {
	if err, ok := e.v.(error); ok {
		return err.Error()
	}
	if s, ok := e.v.(string); ok {
		return s
	}
	return "internal panic"
}

// parseWith answers whether src is in the proto language — and,
// when the format's value is JSON-representable, what it parsed to.
//
// A rejection is an ANSWER, not a failure of the call: ok:true with
// accept:false. ok:false is reserved for the call itself being wrong —
// an unknown handle, an engine bug — so a caller can tell "your input
// is not in the language" from "you called me wrong" without reading
// messages.
func parseWith(handle int64, src string) string {
	reg.RLock()
	g := loaded[handle]
	reg.RUnlock()
	if g == nil {
		return failDoc("handle", "no parser is loaded under that handle")
	}

	g.mu.Lock()
	val, err := safeParse(g.parse, src)
	g.mu.Unlock()

	if err != nil {
		if _, isPanic := err.(*panicErr); isPanic {
			return failDoc("internal", firstLine(err.Error()))
		}
		return reply(map[string]any{
			"ok": true, "accept": false, "error": errPayload(err),
		})
	}

	doc := map[string]any{"ok": true, "accept": true}
	if valueOut {
		if b, jerr := json.Marshal(val); jerr == nil {
			doc["value"] = json.RawMessage(b)
		} else {
			doc["valueError"] = firstLine(jerr.Error())
		}
	}
	return reply(doc)
}

func safeParse(p parseFn, src string) (val any, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = &panicErr{r}
		}
	}()
	return p(src)
}

func freeGrammar(handle int64) {
	reg.Lock()
	delete(loaded, handle)
	reg.Unlock()
}
