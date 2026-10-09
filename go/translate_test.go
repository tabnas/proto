/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

package tabnasproto

import (
	"encoding/json"
	"os"
	"testing"
)

// The parts a host sees are the package's embedded copies, written by
// `npm run embed` in ts/; they must be the repository's files.
func TestTranslationParts(t *testing.T) {
	parts := Translate()
	if parts == nil {
		t.Fatal("Translate returned nil")
	}
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Manifest != string(manifest) {
		t.Fatal("embedded manifest differs from tabnas.plugin.json: run npm run embed in ts")
	}
	if parts.Lift != nil {
		t.Fatal("proto has no lift")
	}
	if parts.Render == nil || parts.Render.Entry != "proto-render" {
		t.Fatalf("render entry is %#v", parts.Render)
	}
	render, err := os.ReadFile("../alchemy/render.alc")
	if err != nil {
		t.Fatal(err)
	}
	if parts.Render.Source != string(render) {
		t.Fatal("embedded render differs from alchemy/render.alc: run npm run embed in ts")
	}
}

// An embed takes a plain tree into a format's own schema. proto's render
// writes the descriptor the reader builds and nothing else, so its
// manifest names no embed and the package carries none; a manifest that
// named one would be held to its file here, as the render is above.
func TestTranslationEmbed(t *testing.T) {
	manifest, err := os.ReadFile("../tabnas.plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	var spec struct {
		Translate struct {
			Embed *string `json:"embed"`
		} `json:"translate"`
	}
	if err := json.Unmarshal(manifest, &spec); err != nil {
		t.Fatal(err)
	}
	parts := Translate()
	if spec.Translate.Embed == nil {
		if parts.Embed != nil {
			t.Fatalf("the manifest names no embed, and Translate carries %#v", parts.Embed)
		}
		return
	}
	if parts.Embed == nil || parts.Embed.Entry != "proto-embed" {
		t.Fatalf("embed entry is %#v", parts.Embed)
	}
	embed, err := os.ReadFile("../" + *spec.Translate.Embed)
	if err != nil {
		t.Fatal(err)
	}
	if parts.Embed.Source != string(embed) {
		t.Fatalf("embedded embed differs from %s", *spec.Translate.Embed)
	}
}

// The tree is the descriptor's own shape, so the manifest names it.
func TestTranslationSchema(t *testing.T) {
	var spec struct {
		Translate struct {
			Reads  string `json:"reads"`
			Writes string `json:"writes"`
			Root   string `json:"root"`
			Schema string `json:"schema"`
		} `json:"translate"`
	}
	if err := json.Unmarshal([]byte(Translate().Manifest), &spec); err != nil {
		t.Fatal(err)
	}
	if spec.Translate.Reads != "tree" || spec.Translate.Writes != "tree" || spec.Translate.Root != "object" || spec.Translate.Schema != "proto-descriptor" {
		t.Fatalf("translate is %+v", spec.Translate)
	}
}
