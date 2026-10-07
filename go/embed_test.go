package tabnasxml

import (
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

const wantEmbedRefusal = "xml: embed mode needs a jsonic host: install the xml plugin on a jsonic engine"

// Embed mode splices XML into jsonic's `val` rule, so it needs a jsonic
// host. On the bare engine the plugin must refuse to install, rather than
// build a parser that turns every document into nothing.
func TestEmbedNeedsAJsonicHost(t *testing.T) {
	j := tabnas.Make()
	err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true})
	if err == nil || !strings.HasPrefix(err.Error(), wantEmbedRefusal) {
		t.Fatalf("embed on the bare engine: got %v, want an error starting %q", err, wantEmbedRefusal)
	}

	// Installing jsonic AFTER the plugin is the same mistake: the plugin
	// refuses first, and the jsonic installed next gets no XML rules.
	if err := j.Use(jsonic.Grammar); err != nil {
		t.Fatalf("jsonic after the refused plugin: %v", err)
	}
	if j.RSM()["element"] != nil {
		t.Fatalf("the refused plugin installed its rules")
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

// A `val` rule alone is not enough: a strict-JSON host has one, and so may
// any grammar. The plugin looks for jsonic's relaxed alternates on `val`,
// the ones in the group "jsonic", live under the host's Rule.Include and
// Rule.Exclude.
func TestEmbedNeedsJsonicsVal(t *testing.T) {
	other := func(j *tabnas.Tabnas, _ map[string]any) error {
		st := j.Token("#ST")
		j.Rule("val", func(rs *tabnas.RuleSpec, _ *tabnas.Parser) {
			rs.AddOpen(&tabnas.AltSpec{S: [][]tabnas.Tin{{st}}, G: "other"})
		})
		return nil
	}
	hosts := []struct {
		name string
		make func() *tabnas.Tabnas
	}{
		// jsonic's strict-JSON parser: the @tabnas/json core, with
		// jsonic's alternates shed by Include "json".
		{"jsonic.MakeJSON()", jsonic.MakeJSON},
		// jsonic with the "jsonic" group excluded.
		{"jsonic.Make(Exclude jsonic)", func() *tabnas.Tabnas {
			return jsonic.Make(tabnas.Options{Rule: &tabnas.RuleOptions{Exclude: "jsonic"}})
		}},
		// The same, with the grammar installed after the option. The
		// alternates the option excludes are still on the rule here, so
		// the check reads the option too.
		{"Exclude jsonic, then jsonic.Grammar", func() *tabnas.Tabnas {
			j := tabnas.Make(tabnas.Options{Rule: &tabnas.RuleOptions{Exclude: "jsonic"}})
			if err := j.Use(jsonic.Grammar); err != nil {
				t.Fatalf("jsonic.Grammar: %v", err)
			}
			return j
		}},
		// Any other grammar that defines `val`.
		{"another grammar's val", func() *tabnas.Tabnas {
			j := tabnas.Make()
			if err := j.Use(other); err != nil {
				t.Fatalf("other grammar: %v", err)
			}
			return j
		}},
	}
	for _, h := range hosts {
		j := h.make()
		if j.RSM()["val"] == nil {
			t.Fatalf("%s: the host has no val rule, so it tests nothing", h.name)
		}
		err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true})
		if err == nil || !strings.HasPrefix(err.Error(), wantEmbedRefusal) {
			t.Fatalf("%s: got %v, want an error starting %q", h.name, err, wantEmbedRefusal)
		}
		if j.RSM()["element"] != nil {
			t.Fatalf("%s: the refused plugin installed its rules", h.name)
		}
	}

	// The refusal changes nothing: the strict-JSON host still parses JSON.
	strict := jsonic.MakeJSON()
	_ = strict.UseDefaults(Xml, Defaults, map[string]any{"embed": true})
	got, err := strict.Parse(`{"a":[1]}`)
	if err != nil {
		t.Fatalf("strict JSON after the refusal: %v", err)
	}
	if b, ok := asMap(got)["a"].([]any); !ok || len(b) != 1 || b[0] != float64(1) {
		t.Fatalf("strict JSON after the refusal: got %#v", got)
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

// Every way of making a jsonic host passes the check. RegisterJsonicGrammar
// registers no plugin, so a check on installed plugins would refuse it;
// dropping a group other than "jsonic" keeps the host jsonic.
func TestEmbedOnEveryJsonicHost(t *testing.T) {
	hosts := []struct {
		name string
		make func() *tabnas.Tabnas
	}{
		{"tabnas.Make(), then jsonic.Grammar", func() *tabnas.Tabnas {
			j := tabnas.Make()
			if err := j.Use(jsonic.Grammar); err != nil {
				t.Fatalf("jsonic.Grammar: %v", err)
			}
			return j
		}},
		{"jsonic.RegisterJsonicGrammar", func() *tabnas.Tabnas {
			j := tabnas.Make()
			if err := jsonic.RegisterJsonicGrammar(j); err != nil {
				t.Fatalf("RegisterJsonicGrammar: %v", err)
			}
			return j
		}},
		{"jsonic.Make().Derive()", func() *tabnas.Tabnas {
			j, err := jsonic.Make().Derive()
			if err != nil {
				t.Fatalf("Derive: %v", err)
			}
			return j
		}},
		{"jsonic.Make(Exclude imp)", func() *tabnas.Tabnas {
			return jsonic.Make(tabnas.Options{Rule: &tabnas.RuleOptions{Exclude: "imp"}})
		}},
	}
	for _, h := range hosts {
		j := h.make()
		if err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true}); err != nil {
			t.Fatalf("%s: %v", h.name, err)
		}
		got, err := j.Parse(`{x: <b>t</b>}`)
		if err != nil {
			t.Fatalf("%s: %v", h.name, err)
		}
		if el, ok := asMap(got)["x"].(map[string]any); !ok || el["name"] != "b" {
			t.Fatalf("%s: got %#v", h.name, got)
		}
	}
}
