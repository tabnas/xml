package tabnasxml

import (
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

// Derive builds a child that re-runs the parent's plugins with the options
// they were installed with, so a derived instance keeps the parent's xml
// options and parses as the parent does. The TypeScript make()
// (ts/test/xml.test.ts `derived-instances`) and the Rust derive
// (rs/tests/xml_test.rs `derive_keeps_the_xml_options`) are held to the
// same.
func TestDeriveKeepsTheXmlOptions(t *testing.T) {
	parent := tabnas.Make()
	if err := parent.UseDefaults(Xml, Defaults, map[string]any{
		"strictNamespaces": true,
		"strictEntities":   false,
		"customEntities":   map[string]string{"copy": "©"},
	}); err != nil {
		t.Fatalf("install: %v", err)
	}
	child, err := parent.Derive()
	if err != nil {
		t.Fatalf("Derive: %v", err)
	}
	grandchild, err := child.Derive()
	if err != nil {
		t.Fatalf("Derive of the child: %v", err)
	}
	for name, j := range map[string]*tabnas.Tabnas{"child": child, "grandchild": grandchild} {
		_, err := j.Parse(`<a><p:b/></a>`)
		if te, ok := err.(*tabnas.TabnasError); !ok || te.Code != "unbound_prefix" {
			t.Fatalf("%s: unbound prefix: got %v, want unbound_prefix", name, err)
		}
		got, err := j.Parse(`<a>&copy;&nope;</a>`)
		if err != nil {
			t.Fatalf("%s: entities: %v", name, err)
		}
		kids, _ := asMap(got)["children"].([]any)
		if len(kids) != 1 || kids[0] != "©&nope;" {
			t.Fatalf("%s: entities: got %#v", name, got)
		}
	}
}

func TestDeriveKeepsEmbedMode(t *testing.T) {
	parent := jsonic.Make()
	if err := parent.UseDefaults(Xml, Defaults, map[string]any{"embed": true}); err != nil {
		t.Fatalf("install: %v", err)
	}
	child, err := parent.Derive()
	if err != nil {
		t.Fatalf("Derive: %v", err)
	}
	grandchild, err := child.Derive()
	if err != nil {
		t.Fatalf("Derive of the child: %v", err)
	}
	for name, j := range map[string]*tabnas.Tabnas{"child": child, "grandchild": grandchild} {
		got, err := j.Parse(`{x: <b>t</b>}`)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		el, ok := asMap(got)["x"].(map[string]any)
		if !ok || el["name"] != "b" {
			t.Fatalf("%s: got %#v", name, got)
		}
	}
}
