package tabnasxml

import (
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

// Embed mode splices XML into jsonic's `val` rule, so it needs a jsonic
// host. On the bare engine the plugin must refuse to install, rather than
// build a parser that turns every document into nothing.
func TestEmbedNeedsAJsonicHost(t *testing.T) {
	const want = "xml: embed mode needs a jsonic host: install the xml plugin on a jsonic engine"

	j := tabnas.Make()
	err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true})
	if err == nil || !strings.HasPrefix(err.Error(), want) {
		t.Fatalf("embed on the bare engine: got %v, want an error starting %q", err, want)
	}

	// Pure mode needs no jsonic, and embed: false is pure mode.
	p := tabnas.Make()
	if err := p.UseDefaults(Xml, Defaults, map[string]any{"embed": false}); err != nil {
		t.Fatalf("pure mode on the bare engine: %v", err)
	}
	got, err := p.Parse(`<a/>`)
	if err != nil {
		t.Fatalf("pure mode parse: %v", err)
	}
	if el, ok := got.(map[string]any); !ok || el["name"] != "a" {
		t.Fatalf("pure mode parse: got %#v", got)
	}
}

func TestEmbedOnAJsonicHost(t *testing.T) {
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true}); err != nil {
		t.Fatalf("embed on a jsonic host: %v", err)
	}

	// Plain jsonic is unaffected.
	got, err := j.Parse(`{a:1, b:"two"}`)
	if err != nil {
		t.Fatalf("plain jsonic: %v", err)
	}
	m := asMap(got)
	if m["a"] != float64(1) || m["b"] != "two" {
		t.Fatalf("plain jsonic: got %#v", got)
	}

	// An XML literal is a value, here inside a map.
	got, err = j.Parse(`{x: <b>t</b>}`)
	if err != nil {
		t.Fatalf("xml literal in a map: %v", err)
	}
	el, ok := asMap(got)["x"].(map[string]any)
	if !ok || el["name"] != "b" {
		t.Fatalf("xml literal in a map: got %#v", got)
	}
	if kids, ok := el["children"].([]any); !ok || len(kids) != 1 || kids[0] != "t" {
		t.Fatalf("xml literal children: got %#v", el["children"])
	}
}
