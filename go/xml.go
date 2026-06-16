// Copyright (c) 2021-2025 Richard Rodger, MIT License

// Package xml is a Jsonic plugin that parses XML into a tree of
// elements. The parser supports: elements with open/close and
// self-closing tags, attributes (single and double quoted with entity
// decoding), mixed element/text content, predefined and numeric
// character entity references, namespace resolution from xmlns/xmlns:*
// declarations, comments, CDATA sections, processing instructions and
// DOCTYPE declarations.
//
// The returned tree uses `map[string]any` nodes with keys `name`,
// `localName`, optional `prefix`, optional `namespace`, `attributes`
// (map of string -> string) and `children` (array of nested elements
// or text strings).
package xml

import (
	"encoding/binary"
	"fmt"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf16"
	"unicode/utf8"

	jsonic "github.com/tabnas/jsonic/go"
)

const Version = "0.1.1"

// Defaults are merged with caller-supplied options when the plugin is
// registered via jsonic.UseDefaults.
//
// Option keys:
//
//	namespaces     bool              resolve xmlns / xmlns:* into prefix /
//	                                 localName / namespace fields on every
//	                                 element. Default: true.
//	entities       bool              decode the five predefined entities and
//	                                 numeric character references in text and
//	                                 attribute values. Default: true.
//	customEntities map[string]string extra named entities to recognise.
//	strictEntities bool              enforce XML 1.0 §4.1: every named entity
//	                                 reference must resolve to a declared
//	                                 entity. Default: true. When false,
//	                                 references to unknown names are left
//	                                 as-is in the output.
//	embed          bool              when true, keep Jsonic's JSON/JSONIC
//	                                 grammar in place and splice an XML
//	                                 literal alternate into the `val` rule
//	                                 so `<tag>…</tag>` can appear wherever
//	                                 Jsonic expects a value. When false
//	                                 (default) the parser is reconfigured
//	                                 as a pure-XML parser.
var Defaults = map[string]any{
	"namespaces":     true,
	"entities":       true,
	"customEntities": map[string]string{},
	"strictEntities": true,
	"embed":          false,
}

// Xml is the Jsonic plugin entry point. Register via:
//
//	j := jsonic.Make()
//	j.UseDefaults(xml.Xml, xml.Defaults)
//	result, err := j.Parse(src)
func Xml(j *jsonic.Jsonic, options map[string]any) error {
	// Guard against re-invocation: Use() re-runs plugins on SetOptions calls.
	if j.Decoration("xml-init") != nil {
		return nil
	}
	j.Decorate("xml-init", true)

	namespacesOn := toBool(options["namespaces"], true)
	entitiesOn := toBool(options["entities"], true)
	customEntities := toStringMap(options["customEntities"])
	strictEntities := toBool(options["strictEntities"], true)
	embed := toBool(options["embed"], false)

	decode, declared := buildEntityDecoder(entitiesOn, customEntities)

	// Reserve #XIG (ignored) and #XOP/#XCL/#XSC (tag tokens) so they have
	// stable tins before the grammar references them. The tins are then
	// passed to the tag matcher by closure.
	xigTin := j.Token("#XIG", "")
	xopTin := j.Token("#XOP", "")
	xclTin := j.Token("#XCL", "")
	xscTin := j.Token("#XSC", "")

	if !embed {
		// Register a dummy fixed token bound to a character that cannot
		// legally appear in XML source (ASCII SOH). This keeps the
		// lexer's internal `FixedSorted` list non-empty, which in turn
		// disables an otherwise-hardcoded fallback that still ends text
		// tokens on `{ } [ ] : ,` even when those symbols have been
		// removed from the fixed token map. Without this, XML text
		// content containing a comma would be truncated at the comma.
		// In embed mode the JSON structural tokens remain in place, so
		// the dummy is not needed.
		soh := "\x01"
		_ = j.Token("#XDUM", soh)
	}

	// Shared options installed in both modes: the custom matcher, the
	// text-end character `<`, and the XML-specific error templates.
	j.SetOptions(jsonic.Options{
		Lex: &jsonic.LexOptions{
			Match: map[string]*jsonic.MatchSpec{
				"xmltag": {Order: 100_000, Make: buildXmlTagMatcher(decode, declared, entitiesOn, strictEntities, embed, xigTin, xopTin, xclTin, xscTin)},
			},
		},
		Ender: []string{"<"},
		Error: map[string]string{
			"xml_mismatched_tag":       "closing tag </$fsrc> does not match opening tag <$openname>",
			"xml_invalid_tag":          "invalid tag: $fsrc",
			"xml_unterminated":         "unterminated $kind",
			"comment_double_dash":      "comment body cannot contain \"--\"",
			"cdata_terminator_in_text": "character data cannot contain \"]]>\"",
			"pi_target_invalid":        "processing instruction target is missing or invalid",
			"lt_in_attr_value":         "\"<\" is not allowed in an attribute value",
			"bad_entity_ref":           "malformed entity reference (need &name; or &#NNN; or &#xHHH;)",
			"duplicate_attribute":      "duplicate attribute name in tag",
			"invalid_xml_char":         "illegal control character in XML data",
			"reserved_namespace":       "invalid use of a reserved namespace prefix or URI",
			"unbound_prefix":           "element or attribute uses an undeclared namespace prefix",
			"undeclared_entity":        "reference to undeclared entity",
		},
		Hint: map[string]string{
			"xml_mismatched_tag":       "Each opening tag must be paired with a matching closing tag.\nExpected </$openname> but found </$fsrc>.",
			"xml_invalid_tag":          "The tag syntax is not valid XML.",
			"xml_unterminated":         "The $kind starting at this position is not terminated.",
			"comment_double_dash":      "XML 1.0 disallows \"--\" inside a comment body.",
			"cdata_terminator_in_text": "The literal \"]]>\" must only appear as the end of a CDATA section.",
			"pi_target_invalid":        "A processing instruction must start with a Name; the XML declaration <?xml...?> is the special case.",
			"lt_in_attr_value":         "Use the entity reference &lt; to include \"<\" in an attribute value.",
			"bad_entity_ref":           "Replace literal \"&\" with &amp;, or terminate the entity reference with \";\".",
			"duplicate_attribute":      "Each attribute name in an open tag must be unique.",
			"invalid_xml_char":         "Only #x9, #xA, #xD and code points >= #x20 are legal XML characters.",
			"reserved_namespace":       "The \"xml\" prefix is fixed to " + xmlNSURI + "; the \"xmlns\" prefix cannot be redeclared, and neither URI may be bound to any other prefix or as the default namespace.",
			"unbound_prefix":           "Declare the prefix with xmlns:prefix=\"...\" on this element or one of its ancestors.",
			"undeclared_entity":        "Declare the entity in the DOCTYPE internal subset, add it to the customEntities option, or set strictEntities: false to allow unresolved references through.",
		},
	})

	if !embed {
		// Pure XML mode: reconfigure the parser so Jsonic's own value
		// grammar is unreachable and all lexers other than our tag
		// matcher are quiescent.
		//
		// Note: we deliberately do NOT install a Text.Modify hook
		// here. While the root element is open the custom matcher
		// itself emits the text tokens (with entity decoding and
		// well-formedness checks); Jsonic's text matcher only sees
		// whitespace before and after the root element where no
		// decoding is needed.
		j.SetOptions(jsonic.Options{
			Rule: &jsonic.RuleOptions{
				Start:   "xml",
				Exclude: "jsonic,imp",
			},
			Fixed: &jsonic.FixedOptions{Token: map[string]*string{
				"#OB": nil, "#CB": nil, "#OS": nil, "#CS": nil,
				"#CL": nil, "#CA": nil,
			}},
			Number:  &jsonic.NumberOptions{Lex: boolPtr(false)},
			Value:   &jsonic.ValueOptions{Lex: boolPtr(false)},
			String:  &jsonic.StringOptions{Lex: boolPtr(false)},
			Comment: &jsonic.CommentOptions{Lex: boolPtr(false)},
			Space:   &jsonic.SpaceOptions{Lex: boolPtr(false)},
			Line:    &jsonic.LineOptions{Lex: boolPtr(false)},
		})
	}

	// IGNORE set: drop #XIG (comments, PIs, DOCTYPE) along with the
	// default members so any of them is skipped by the parser. In
	// embed mode this preserves all default ignored tokens; in pure
	// mode the SP/LN/CM tokens are never produced (we disabled their
	// lexers), but keeping them here is harmless.
	j.SetTokenSet("IGNORE", []jsonic.Tin{
		j.Token("#SP", ""), j.Token("#LN", ""), j.Token("#CM", ""), xigTin,
	})

	// Grammar declarations. Mirror the TypeScript grammar exactly.
	refs := map[jsonic.FuncRef]any{
		"@xml-bc": jsonic.StateAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if r.Child == nil || r.Child == jsonic.NoRule || r.Child.Node == nil {
				return
			}
			// The Go parser follows the Next chain forward from the root
			// rule to find the final result holder, so the current rule's
			// node is what the caller will see. Set it (and the original
			// root's node via the Prev chain as well for safety).
			r.Node = r.Child.Node
			root := firstRule(r)
			root.Node = r.Child.Node
			// Mark the document as having seen its root so the
			// @no-root-yet condition rejects any subsequent attempt
			// to push a second root element (XML 1.0 §2.1).
			ctx.U["rootSeen"] = true
			if namespacesOn {
				if el, ok := r.Node.(map[string]any); ok {
					if code := resolveNamespaces(el, nil); code != "" {
						ctx.ParseErr = &jsonic.Token{
							Name: "#BD", Tin: jsonic.TinBD,
							Err: code, Why: code, Src: code,
						}
					}
				}
			}
		}),

		"@no-root-yet": jsonic.AltCond(func(_ *jsonic.Rule, ctx *jsonic.Context) bool {
			seen, _ := ctx.U["rootSeen"].(bool)
			return !seen
		}),

		"@element-open": jsonic.AltAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			v := r.O0.Val.(map[string]any)
			name := v["name"].(string)
			attrs := v["attributes"].(map[string]any)
			r.Node = map[string]any{
				"name":       name,
				"localName":  name,
				"attributes": applyAttrDefaults(attrs, name, ctx),
				"children":   []any{},
			}
		}),

		"@element-selfclose": jsonic.AltAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			v := r.O0.Val.(map[string]any)
			name := v["name"].(string)
			attrs := v["attributes"].(map[string]any)
			r.Node = map[string]any{
				"name":       name,
				"localName":  name,
				"attributes": applyAttrDefaults(attrs, name, ctx),
				"children":   []any{},
			}
		}),

		"@element-close": jsonic.AltAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			el, _ := r.Node.(map[string]any)
			openName, _ := el["name"].(string)
			closeName, _ := r.C0.Val.(string)
			if openName != closeName {
				// The Go parser's top-level error handling reports parse
				// errors under a single "unexpected" code, so encode our
				// specific error code into the token's `Src`: that string
				// is substituted into the error detail via $fsrc and will
				// appear in err.Error() for consumers (and tests) that
				// want to key on the specific cause.
				r.C0.Src = "xml_mismatched_tag: </" + closeName + "> does not match <" + openName + ">"
				if r.C0.Use == nil {
					r.C0.Use = map[string]any{}
				}
				r.C0.Use["openname"] = openName
				r.C0.Err = "xml_mismatched_tag"
				ctx.ParseErr = r.C0
			}
		}),

		"@child-text": jsonic.AltAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			el, _ := r.Node.(map[string]any)
			children, _ := el["children"].([]any)
			el["children"] = append(children, r.O0.Val)
			r.U["done"] = true
		}),

		"@child-bc": jsonic.StateAction(func(r *jsonic.Rule, ctx *jsonic.Context) {
			if done, _ := r.U["done"].(bool); done {
				return
			}
			if r.Child == nil || r.Child == jsonic.NoRule || r.Child.Node == nil {
				return
			}
			el, ok := r.Node.(map[string]any)
			if !ok {
				return
			}
			children, _ := el["children"].([]any)
			el["children"] = append(children, r.Child.Node)
		}),

		"@element-is-selfclosed": jsonic.AltCond(func(r *jsonic.Rule, ctx *jsonic.Context) bool {
			v, _ := r.U["selfclose"].(int)
			return v == 1
		}),
	}

	gs := &jsonic.GrammarSpec{
		Ref: refs,
		Rule: map[string]*jsonic.GrammarRuleSpec{
			"xml": {
				Open: []*jsonic.GrammarAltSpec{
					{S: "#ZZ"},
					{S: "#TX", R: "xml"},
					{P: "element", C: "@no-root-yet"},
				},
				Close: []*jsonic.GrammarAltSpec{
					{S: "#ZZ"},
					{S: "#TX", R: "xml"},
				},
			},
			"element": {
				Open: []*jsonic.GrammarAltSpec{
					{S: "#XSC", A: "@element-selfclose", U: map[string]any{"selfclose": 1}},
					{S: "#XOP", P: "content", A: "@element-open"},
				},
				Close: []*jsonic.GrammarAltSpec{
					{C: "@element-is-selfclosed"},
					{S: "#XCL", A: "@element-close"},
				},
			},
			"content": {
				Open: []*jsonic.GrammarAltSpec{
					{S: "#XCL", B: 1},
					{P: "child"},
				},
				Close: []*jsonic.GrammarAltSpec{
					{S: "#XCL", B: 1},
					{R: "content"},
				},
			},
			"child": {
				Open: []*jsonic.GrammarAltSpec{
					{S: "#TX", A: "@child-text"},
					{S: "#XOP", B: 1, P: "element"},
					{S: "#XSC", B: 1, P: "element"},
				},
			},
		},
	}
	if err := j.Grammar(gs); err != nil {
		return fmt.Errorf("xml: apply grammar: %w", err)
	}

	if embed {
		// Splice XML literals into the Jsonic `val` rule. When the
		// parser is looking for a value and sees `#XOP` or `#XSC`,
		// push the `element` rule (backtracking by 1 so element.open
		// can read the same token and dispatch).
		j.Rule("val", func(rs *jsonic.RuleSpec, _ *jsonic.Parser) {
			rs.AddOpen(
				&jsonic.AltSpec{
					S: [][]jsonic.Tin{{xopTin}},
					B: 1, P: "element", G: "xml",
				},
				&jsonic.AltSpec{
					S: [][]jsonic.Tin{{xscTin}},
					B: 1, P: "element", G: "xml",
				},
			)
		})

		// In embed mode the top-level wrapper is Jsonic's `val` rule,
		// so the @xml-bc hook that copies the root element to
		// ctx.root().node is not invoked. Resolve namespaces instead
		// when the element rule closes directly under a val rule.
		if namespacesOn {
			j.Rule("element", func(rs *jsonic.RuleSpec, _ *jsonic.Parser) {
				rs.AddBC(func(r *jsonic.Rule, ctx *jsonic.Context) {
					if r.Parent != nil && r.Parent != jsonic.NoRule &&
						r.Parent.Name == "val" {
						if el, ok := r.Node.(map[string]any); ok {
							resolveNamespaces(el, nil)
						}
					}
				})
			})
		}
	} else {
		// Pure XML mode: the `xml` start rule reaches only the XML rules
		// (element/content/child), so Jsonic's inherited JSON value rules
		// are unreachable. Remove them so the grammar definition matches
		// the TypeScript port (Rule(name, nil) deletes the rule).
		for _, name := range []string{"val", "map", "list", "pair", "elem"} {
			j.Rule(name, nil)
		}
	}

	return nil
}

// dtdEntities reads the per-parse DOCTYPE-declared entity map (set
// by the DOCTYPE matcher path). Returns nil if none have been
// registered yet.
func dtdEntities(lex *jsonic.Lex) map[string]string {
	if lex == nil || lex.Ctx == nil || lex.Ctx.U == nil {
		return nil
	}
	m, _ := lex.Ctx.U["dtdEntities"].(map[string]string)
	return m
}

// dtdAttrDefaults reads the per-parse DOCTYPE-supplied attribute
// default map keyed by element name (set by the DOCTYPE matcher
// path). Returns nil if none have been registered yet.
func dtdAttrDefaults(ctx *jsonic.Context) map[string]map[string]string {
	if ctx == nil || ctx.U == nil {
		return nil
	}
	m, _ := ctx.U["dtdAttrDefaults"].(map[string]map[string]string)
	return m
}

// applyAttrDefaults merges in DOCTYPE-supplied default attribute
// values for any attribute missing from the parsed element instance.
// Returns the original map if no defaults apply.
func applyAttrDefaults(
	attrs map[string]any, elemName string, ctx *jsonic.Context,
) map[string]any {
	all := dtdAttrDefaults(ctx)
	if all == nil {
		return attrs
	}
	defaults, ok := all[elemName]
	if !ok {
		return attrs
	}
	for k, v := range defaults {
		if _, present := attrs[k]; !present {
			attrs[k] = v
		}
	}
	return attrs
}

// parseDoctypeAttlists scans a DOCTYPE internal-subset body and
// extracts every `<!ATTLIST element attr type defaultDecl>` default
// attribute value, keyed by element name and attribute name. Both
// literal defaults and `#FIXED "value"` defaults are returned;
// `#REQUIRED` and `#IMPLIED` declarations contribute nothing because
// they have no default value.
func parseDoctypeAttlists(body string) map[string]map[string]string {
	skipSpace := func(s int) int {
		for s < len(body) && isSpace(body[s]) {
			s++
		}
		return s
	}
	out := map[string]map[string]string{}

	i := 0
	for i < len(body) {
		idx := strings.Index(body[i:], "<!ATTLIST")
		if idx < 0 {
			break
		}
		j := i + idx + len("<!ATTLIST")
		j = skipSpace(j)
		elemName, after, ok := readName(body, j)
		if !ok {
			i = j + 1
			continue
		}
		j = after

		for j < len(body) {
			j = skipSpace(j)
			if j >= len(body) {
				break
			}
			if body[j] == '>' {
				j++
				break
			}
			attrName, attrEnd, ok := readName(body, j)
			if !ok {
				j++
				continue
			}
			j = attrEnd
			j = skipSpace(j)

			// Skip AttType.
			if j < len(body) && body[j] == '(' {
				close := strings.Index(body[j:], ")")
				if close < 0 {
					j = len(body)
					break
				}
				j = j + close + 1
			} else if strings.HasPrefix(body[j:], "NOTATION") {
				j += len("NOTATION")
				j = skipSpace(j)
				if j < len(body) && body[j] == '(' {
					close := strings.Index(body[j:], ")")
					if close < 0 {
						j = len(body)
						break
					}
					j = j + close + 1
				}
			} else {
				for j < len(body) && body[j] >= 'A' && body[j] <= 'Z' {
					j++
				}
			}
			j = skipSpace(j)

			// DefaultDecl.
			if strings.HasPrefix(body[j:], "#REQUIRED") {
				j += len("#REQUIRED")
				continue
			}
			if strings.HasPrefix(body[j:], "#IMPLIED") {
				j += len("#IMPLIED")
				continue
			}
			if strings.HasPrefix(body[j:], "#FIXED") {
				j += len("#FIXED")
				j = skipSpace(j)
			}
			if j < len(body) && (body[j] == '"' || body[j] == '\'') {
				quote := body[j]
				j++
				valStart := j
				for j < len(body) && body[j] != quote {
					j++
				}
				if j >= len(body) {
					break
				}
				value := body[valStart:j]
				if out[elemName] == nil {
					out[elemName] = map[string]string{}
				}
				out[elemName][attrName] = value
				j++
			}
		}
		i = j
	}
	return out
}

// parseDoctypeEntities scans a DOCTYPE internal-subset body and
// extracts every internal general entity declaration of the form
// `<!ENTITY name "value">` (or single-quoted). Parameter entity
// declarations (`<!ENTITY % name ...>`) and external entity
// declarations (`<!ENTITY name SYSTEM "...">` etc.) are skipped, as
// are `<!ELEMENT`, `<!ATTLIST`, and `<!NOTATION` declarations.
//
// Returned values are stored verbatim — character and entity
// references inside an entity value are expanded only when the
// outer entity is referenced.
func parseDoctypeEntities(body string) map[string]string {
	out := map[string]string{}
	i := 0
	for i < len(body) {
		idx := strings.Index(body[i:], "<!ENTITY")
		if idx < 0 {
			break
		}
		j := i + idx + len("<!ENTITY")
		for j < len(body) && isSpace(body[j]) {
			j++
		}
		// Parameter entity: skip.
		if j < len(body) && body[j] == '%' {
			end := strings.Index(body[j:], ">")
			if end < 0 {
				break
			}
			i = j + end + 1
			continue
		}
		// Read the entity name.
		name, after, ok := readName(body, j)
		if !ok {
			i = j + 1
			continue
		}
		j = after
		for j < len(body) && isSpace(body[j]) {
			j++
		}
		// Quoted value -> internal entity. SYSTEM/PUBLIC -> skip.
		if j < len(body) && (body[j] == '"' || body[j] == '\'') {
			quote := body[j]
			j++
			valStart := j
			for j < len(body) && body[j] != quote {
				j++
			}
			if j >= len(body) {
				break
			}
			out[name] = body[valStart:j]
			j++
		}
		end := strings.Index(body[j:], ">")
		if end < 0 {
			break
		}
		i = j + end + 1
	}
	return out
}

// xmlDepth reads the per-parse XML nesting counter from the lex context.
// Returns 0 if not set.
func xmlDepth(lex *jsonic.Lex) int {
	if lex == nil || lex.Ctx == nil {
		return 0
	}
	if lex.Ctx.U == nil {
		lex.Ctx.U = map[string]any{}
		return 0
	}
	v, _ := lex.Ctx.U["xmlDepth"].(int)
	return v
}

// setXmlDepth writes the XML nesting counter, clamping at zero.
func setXmlDepth(lex *jsonic.Lex, d int) {
	if lex == nil || lex.Ctx == nil {
		return
	}
	if lex.Ctx.U == nil {
		lex.Ctx.U = map[string]any{}
	}
	if d < 0 {
		d = 0
	}
	lex.Ctx.U["xmlDepth"] = d
}

// DecodeBOM detects a byte-order mark at the start of `src` and, when
// the input is encoded as UTF-16 LE/BE or UTF-32 LE/BE, returns a
// transcoded UTF-8 string. UTF-8 BOMs are returned with the BOM bytes
// stripped. For input without a recognised BOM, the original string
// is returned unchanged.
//
// Use this when feeding XML files of unknown encoding into the
// parser:
//
//	body, _ := os.ReadFile(path)
//	doc, err := j.Parse(xml.DecodeBOM(string(body)))
func DecodeBOM(src string) string {
	b := []byte(src)
	n := len(b)
	switch {
	case n >= 4 && b[0] == 0x00 && b[1] == 0x00 && b[2] == 0xfe && b[3] == 0xff:
		return decodeUTF32(b[4:], binary.BigEndian)
	case n >= 4 && b[0] == 0xff && b[1] == 0xfe && b[2] == 0x00 && b[3] == 0x00:
		return decodeUTF32(b[4:], binary.LittleEndian)
	case n >= 2 && b[0] == 0xfe && b[1] == 0xff:
		return decodeUTF16(b[2:], binary.BigEndian)
	case n >= 2 && b[0] == 0xff && b[1] == 0xfe:
		return decodeUTF16(b[2:], binary.LittleEndian)
	case n >= 3 && b[0] == 0xef && b[1] == 0xbb && b[2] == 0xbf:
		return string(b[3:])
	}
	return src
}

func decodeUTF16(b []byte, order binary.ByteOrder) string {
	if len(b)%2 != 0 {
		b = b[:len(b)-1]
	}
	units := make([]uint16, len(b)/2)
	for i := range units {
		units[i] = order.Uint16(b[i*2:])
	}
	return string(utf16.Decode(units))
}

func decodeUTF32(b []byte, order binary.ByteOrder) string {
	if len(b)%4 != 0 {
		b = b[:len(b)-(len(b)%4)]
	}
	out := make([]rune, len(b)/4)
	for i := range out {
		out[i] = rune(order.Uint32(b[i*4:]))
	}
	return string(out)
}

// firstRule walks back through Prev links to find the originating rule
// instance (matches the root rule used by the parser as the result
// holder).
func firstRule(r *jsonic.Rule) *jsonic.Rule {
	cur := r
	for cur.Prev != nil && cur.Prev != jsonic.NoRule {
		cur = cur.Prev
	}
	return cur
}

// predefinedEntities is the five XML-predefined entities.
var predefinedEntities = map[string]string{
	"amp":  "&",
	"lt":   "<",
	"gt":   ">",
	"quot": "\"",
	"apos": "'",
}

// entityRE matches a single entity reference: named, decimal numeric, or
// hexadecimal numeric. (?:...) would be ideal but the Go stdlib regexp
// supports named groups; this uses plain groups for portability.
var entityRE = regexp.MustCompile(`&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z_][A-Za-z0-9_]*);`)

// EntityDecoder decodes XML entity references in `s`. The optional
// `dtd` map supplies general entity declarations parsed from the
// DOCTYPE internal subset; values are recursively expanded with
// cycle detection.
type EntityDecoder func(s string, dtd map[string]string) string

// buildEntityDecoder returns a function that decodes the five
// predefined entities, numeric character references, any
// caller-supplied custom entities, and per-parse DTD entities.
// When `enabled` is false the function is an identity. The second
// return value is the merged set of always-declared names used for
// strict-entity validation in the matcher.
func buildEntityDecoder(
	enabled bool, custom map[string]string,
) (EntityDecoder, map[string]string) {
	base := make(map[string]string, len(predefinedEntities)+len(custom))
	for k, v := range predefinedEntities {
		base[k] = v
	}
	for k, v := range custom {
		base[k] = v
	}
	if !enabled {
		return func(s string, _ map[string]string) string { return s }, base
	}
	var expand func(s string, dtd map[string]string, seen map[string]bool) string
	expand = func(s string, dtd map[string]string, seen map[string]bool) string {
		if !strings.Contains(s, "&") {
			return s
		}
		return entityRE.ReplaceAllStringFunc(s, func(match string) string {
			ref := match[1 : len(match)-1]
			if ref[0] == '#' {
				var code int64
				var err error
				if len(ref) > 1 && (ref[1] == 'x' || ref[1] == 'X') {
					code, err = strconv.ParseInt(ref[2:], 16, 32)
				} else {
					code, err = strconv.ParseInt(ref[1:], 10, 32)
				}
				if err != nil {
					return match
				}
				return string(rune(code))
			}
			if v, ok := base[ref]; ok {
				return v
			}
			if dtd != nil {
				if v, ok := dtd[ref]; ok {
					if seen[ref] {
						// Recursive reference; break the cycle.
						return match
					}
					seen[ref] = true
					out := expand(v, dtd, seen)
					delete(seen, ref)
					return out
				}
			}
			return match
		})
	}
	return func(s string, dtd map[string]string) string {
		return expand(s, dtd, map[string]bool{})
	}, base
}

// buildXmlTagMatcher returns a MakeLexMatcher that recognises every
// top-level XML `<...>` construct at the current lex position. On a
// successful match it consumes the full construct and emits exactly
// one of:
//
//	#XOP  <name attr="v" ...>      val = {"name":..., "attributes":...}
//	#XSC  <name attr="v" ... />    val = {"name":..., "attributes":...}
//	#XCL  </name>                  val = name (string)
//	#XIG  <!-- ... -->  |  <?...?>  |  <!DOCTYPE ...>  (ignored)
//	#TX   <![CDATA[ ... ]]>        val = cdata body (verbatim, no entity decoding)
func buildXmlTagMatcher(
	decode EntityDecoder,
	declared map[string]string,
	entitiesOn bool,
	strict bool,
	embed bool,
	xigTin, xopTin, xclTin, xscTin jsonic.Tin,
) jsonic.MakeLexMatcher {
	_ = embed // embed flag is no longer needed for text-handling
	return func(_ *jsonic.LexConfig, _ *jsonic.Options) jsonic.LexMatcher {
		return func(lex *jsonic.Lex, _ *jsonic.Rule) *jsonic.Token {
			pnt := lex.Cursor()
			src := lex.Src
			srclen := len(src)
			sI := pnt.SI

			// Strip a UTF-8 byte-order mark at the very start of input.
			if sI == 0 && srclen >= 3 &&
				src[0] == 0xef && src[1] == 0xbb && src[2] == 0xbf {
				pnt.SI = 3
				return nil
			}

			// Inside an open XML element (depth > 0), consume
			// characters up to the next `<` as a single #TX text
			// token. Validates well-formedness of character data:
			// rejects "]]>" and bare/malformed entity references.
			if sI < srclen && src[sI] != '<' {
				if depth := xmlDepth(lex); depth > 0 {
					i := sI
					for i < srclen && src[i] != '<' {
						i++
					}
					if i == sI {
						return nil
					}
					raw := src[sI:i]
					if code := checkChars(raw); code != "" {
						return lex.Bad(code)
					}
					if strings.Contains(raw, "]]>") {
						return lex.Bad("cdata_terminator_in_text")
					}
					if code := checkEntityRefs(raw, dtdEntities(lex), declared, strict); code != "" {
						return lex.Bad(code)
					}
					// §2.11 end-of-line normalisation.
					normalised := normaliseLineEndings(raw)
					var val any = normalised
					if entitiesOn {
						val = decode(normalised, dtdEntities(lex))
					}
					tkn := lex.Token("#TX", jsonic.TinTX, val, raw)
					advance(pnt, sI, i)
					return tkn
				}
			}

			if sI >= srclen || src[sI] != '<' {
				return nil
			}

			// Comment: <!-- ... -->
			if strings.HasPrefix(src[sI:], "<!--") {
				end := strings.Index(src[sI+4:], "-->")
				if end < 0 {
					return lex.Bad("unterminated_comment")
				}
				bodyStart := sI + 4
				bodyEnd := bodyStart + end
				body := src[bodyStart:bodyEnd]
				// WF: "--" must not occur in a comment body.
				if strings.Contains(body, "--") {
					return lex.Bad("comment_double_dash")
				}
				if code := checkChars(body); code != "" {
					return lex.Bad(code)
				}
				finish := bodyEnd + 3
				tsrc := src[sI:finish]
				tkn := lex.Token("#XIG", xigTin, tsrc, tsrc)
				advance(pnt, sI, finish)
				return tkn
			}

			// CDATA: <![CDATA[ ... ]]>
			if strings.HasPrefix(src[sI:], "<![CDATA[") {
				body := sI + 9
				end := strings.Index(src[body:], "]]>")
				if end < 0 {
					return lex.Bad("unterminated_cdata")
				}
				finish := body + end + 3
				text := src[body : body+end]
				if code := checkChars(text); code != "" {
					return lex.Bad(code)
				}
				tsrc := src[sI:finish]
				// §2.11 line-end normalisation applies to CDATA too.
				tkn := lex.Token("#TX", jsonic.TinTX, normaliseLineEndings(text), tsrc)
				advance(pnt, sI, finish)
				return tkn
			}

			// DOCTYPE: <!DOCTYPE ... [...] > (allows a single level of [] subset)
			if strings.HasPrefix(src[sI:], "<!DOCTYPE") {
				i := sI + 9
				depth := 0
				subsetStart, subsetEnd := -1, -1
				for i < srclen {
					ch := src[i]
					// Skip over quoted strings so `]` and `>` inside an
					// entity value or attribute default cannot terminate
					// the subset prematurely.
					if ch == '"' || ch == '\'' {
						i++
						for i < srclen && src[i] != ch {
							i++
						}
						if i < srclen {
							i++
						}
						continue
					}
					if ch == '[' {
						if depth == 0 {
							subsetStart = i + 1
						}
						depth++
					} else if ch == ']' {
						depth--
						if depth == 0 {
							subsetEnd = i
						}
					} else if ch == '>' && depth <= 0 {
						break
					}
					i++
				}
				if i >= srclen {
					return lex.Bad("unterminated_doctype")
				}
				finish := i + 1
				// Extract internal-subset declarations and stash them
				// on the per-parse context. The matcher's text /
				// attribute paths and the element actions read these
				// back via lex.Ctx.U.
				if subsetStart >= 0 && subsetEnd > subsetStart && lex.Ctx != nil {
					subset := src[subsetStart:subsetEnd]
					if lex.Ctx.U == nil {
						lex.Ctx.U = map[string]any{}
					}
					if found := parseDoctypeEntities(subset); len(found) > 0 {
						existing, _ := lex.Ctx.U["dtdEntities"].(map[string]string)
						if existing == nil {
							existing = map[string]string{}
						}
						for k, v := range found {
							existing[k] = v
						}
						lex.Ctx.U["dtdEntities"] = existing
					}
					if found := parseDoctypeAttlists(subset); len(found) > 0 {
						existing, _ := lex.Ctx.U["dtdAttrDefaults"].(map[string]map[string]string)
						if existing == nil {
							existing = map[string]map[string]string{}
						}
						for elem, defs := range found {
							if existing[elem] == nil {
								existing[elem] = map[string]string{}
							}
							for k, v := range defs {
								existing[elem][k] = v
							}
						}
						lex.Ctx.U["dtdAttrDefaults"] = existing
					}
				}
				tsrc := src[sI:finish]
				tkn := lex.Token("#XIG", xigTin, tsrc, tsrc)
				advance(pnt, sI, finish)
				return tkn
			}

			// Processing instruction: <? ... ?>
			if sI+1 < srclen && src[sI+1] == '?' {
				end := strings.Index(src[sI+2:], "?>")
				if end < 0 {
					return lex.Bad("unterminated_pi")
				}
				bodyEnd := sI + 2 + end
				// WF: PI target must be a Name.
				_, after, ok := readName(src, sI+2)
				if !ok || after > bodyEnd {
					return lex.Bad("pi_target_invalid")
				}
				if after < bodyEnd && !isSpace(src[after]) {
					return lex.Bad("pi_target_invalid")
				}
				if code := checkChars(src[sI+2 : bodyEnd]); code != "" {
					return lex.Bad(code)
				}
				finish := bodyEnd + 2
				tsrc := src[sI:finish]
				tkn := lex.Token("#XIG", xigTin, tsrc, tsrc)
				advance(pnt, sI, finish)
				return tkn
			}

			// Closing tag: </name>
			if sI+1 < srclen && src[sI+1] == '/' {
				name, after, ok := readName(src, sI+2)
				// WF: empty close tag `</>` is invalid.
				if !ok {
					return lex.Bad("xml_invalid_tag")
				}
				i := after
				for i < srclen && isSpace(src[i]) {
					i++
				}
				if i >= srclen || src[i] != '>' {
					return lex.Bad("xml_invalid_tag")
				}
				finish := i + 1
				tsrc := src[sI:finish]
				tkn := lex.Token("#XCL", xclTin, name, tsrc)
				advance(pnt, sI, finish)
				setXmlDepth(lex, xmlDepth(lex)-1)
				return tkn
			}

			// Opening or self-close tag: <name attr="v" ... />
			name, after, ok := readName(src, sI+1)
			if !ok {
				return nil
			}
			i := after
			attrs := map[string]any{}

			for {
				wsStart := i
				for i < srclen && isSpace(src[i]) {
					i++
				}
				if i >= srclen {
					return lex.Bad("xml_invalid_tag")
				}

				// End of tag.
				if src[i] == '>' {
					finish := i + 1
					tsrc := src[sI:finish]
					val := map[string]any{"name": name, "attributes": attrs}
					tkn := lex.Token("#XOP", xopTin, val, tsrc)
					advance(pnt, sI, finish)
					setXmlDepth(lex, xmlDepth(lex)+1)
					return tkn
				}
				if src[i] == '/' && i+1 < srclen && src[i+1] == '>' {
					finish := i + 2
					tsrc := src[sI:finish]
					val := map[string]any{"name": name, "attributes": attrs}
					tkn := lex.Token("#XSC", xscTin, val, tsrc)
					advance(pnt, sI, finish)
					// #XSC is an instantly-closed element; depth unchanged.
					return tkn
				}

				// Attributes must be separated by whitespace.
				if wsStart == i {
					return lex.Bad("xml_invalid_tag")
				}

				// Attribute name.
				attrName, attrEnd, ok := readName(src, i)
				if !ok {
					return lex.Bad("xml_invalid_tag")
				}
				i = attrEnd

				for i < srclen && isSpace(src[i]) {
					i++
				}
				if i >= srclen || src[i] != '=' {
					return lex.Bad("xml_invalid_tag")
				}
				i++
				for i < srclen && isSpace(src[i]) {
					i++
				}

				if i >= srclen {
					return lex.Bad("xml_invalid_tag")
				}
				quote := src[i]
				if quote != '"' && quote != '\'' {
					return lex.Bad("xml_invalid_tag")
				}
				i++
				valStart := i
				// Per the XML 1.0 spec, attribute values cannot contain
				// a literal `<`. Scanning lets us also validate entity
				// references in the value below.
				for i < srclen && src[i] != quote {
					if src[i] == '<' {
						return lex.Bad("lt_in_attr_value")
					}
					i++
				}
				if i >= srclen {
					return lex.Bad("xml_invalid_tag")
				}
				raw := src[valStart:i]
				i++ // consume closing quote

				if code := checkChars(raw); code != "" {
					return lex.Bad(code)
				}
				if code := checkEntityRefs(raw, dtdEntities(lex), declared, strict); code != "" {
					return lex.Bad(code)
				}
				if _, ok := attrs[attrName]; ok {
					return lex.Bad("duplicate_attribute")
				}
				// §3.3.3 attribute-value normalisation: TAB/LF/CR/CRLF
				// all collapse to a single SPACE. Without DTD attribute
				// types, all attributes are treated as CDATA-typed
				// (no further whitespace collapsing or trimming).
				normalised := normaliseAttrWhitespace(raw)
				attrs[attrName] = decode(normalised, dtdEntities(lex))
			}
		}
	}
}

// §2.11 End-of-line handling: any literal CR or CR-LF is normalised
// to a single LF before parsing proceeds. Applies to character data
// and CDATA section bodies.
func normaliseLineEndings(s string) string {
	if !strings.ContainsRune(s, '\r') {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\r' {
			b.WriteByte('\n')
			if i+1 < len(s) && s[i+1] == '\n' {
				i++
			}
		} else {
			b.WriteByte(c)
		}
	}
	return b.String()
}

// §3.3.3 attribute-value normalisation for CDATA-typed attributes:
// TAB / LF / CR / CRLF all collapse to a single SPACE.
func normaliseAttrWhitespace(s string) string {
	if !strings.ContainsAny(s, "\t\n\r") {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch c {
		case '\t', '\n':
			b.WriteByte(' ')
		case '\r':
			b.WriteByte(' ')
			if i+1 < len(s) && s[i+1] == '\n' {
				i++
			}
		default:
			b.WriteByte(c)
		}
	}
	return b.String()
}

// checkChars validates that every byte in `s` is a legal XML 1.0 Char.
// Returns "invalid_xml_char" on the first illegal byte, "" if all
// bytes are legal. Only the C0 control band is checked here; the full
// Char production (which excludes #xFFFE/#xFFFF and unpaired
// surrogates) is not enforced.
func checkChars(s string) string {
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c < 0x20 && c != 0x09 && c != 0x0a && c != 0x0d {
			return "invalid_xml_char"
		}
	}
	return ""
}

// checkEntityRefs validates that every `&` in `s` begins a well-formed
// entity reference. Returns "" on success, otherwise an error code
// suitable for lex.Bad(). The `dtd` map supplies DOCTYPE-declared
// entity names; `declared` adds names that are always declared
// (typically the predefined and caller-supplied entities). When
// `strict` is true, references to unknown names trigger
// "undeclared_entity"; otherwise the syntactic check still runs but
// unknown names pass through.
//
// Well-formed forms:
//
//	&name;     - name must start with a NameStartChar
//	&#nnnn;    - decimal numeric character reference
//	&#xhhhh;   - hexadecimal numeric character reference
func checkEntityRefs(s string, dtd, declared map[string]string, strict bool) string {
	for i := 0; i < len(s); i++ {
		if s[i] != '&' {
			continue
		}
		semi := strings.IndexByte(s[i+1:], ';')
		if semi < 0 {
			return "bad_entity_ref"
		}
		semi += i + 1
		ref := s[i+1 : semi]
		if len(ref) == 0 {
			return "bad_entity_ref"
		}
		if ref[0] == '#' {
			if len(ref) < 2 {
				return "bad_entity_ref"
			}
			var digits string
			var hex bool
			if ref[1] == 'x' || ref[1] == 'X' {
				hex = true
				digits = ref[2:]
			} else {
				digits = ref[1:]
			}
			if len(digits) == 0 {
				return "bad_entity_ref"
			}
			for _, d := range digits {
				if hex {
					if !((d >= '0' && d <= '9') || (d >= 'a' && d <= 'f') || (d >= 'A' && d <= 'F')) {
						return "bad_entity_ref"
					}
				} else {
					if !(d >= '0' && d <= '9') {
						return "bad_entity_ref"
					}
				}
			}
		} else {
			// Entity name must be a Name. Use rune-aware checks so
			// non-ASCII names (Unicode XML 1.0 NameStartChar / NameChar
			// blocks) are accepted.
			r0, sz := utf8.DecodeRuneInString(ref)
			if r0 == utf8.RuneError && sz <= 1 {
				return "bad_entity_ref"
			}
			if !isNameStartRune(r0) {
				return "bad_entity_ref"
			}
			j := sz
			for j < len(ref) {
				r, sz := utf8.DecodeRuneInString(ref[j:])
				if r == utf8.RuneError && sz <= 1 {
					return "bad_entity_ref"
				}
				if !isNameCharRune(r) {
					return "bad_entity_ref"
				}
				j += sz
			}
			// §4.1: in strict mode the named entity must resolve.
			if strict {
				if _, ok := declared[ref]; !ok {
					if _, ok := dtd[ref]; !ok {
						return "undeclared_entity"
					}
				}
			}
		}
		i = semi
	}
	return ""
}

// xmlScope tracks state inherited down an XML tree:
//
//   ns    - prefix -> namespace URI (XML Namespaces 1.0)
//   space - active xml:space value (XML 1.0 §2.10)
//   lang  - active xml:lang value (XML 1.0 §2.12)
//
// `space` and `lang` are recorded on each element only when they
// are non-default, so plain documents don't sprout extra fields.
type xmlScope struct {
	ns    map[string]string
	space string
	lang  string
}

// Reserved namespace URIs (Namespaces in XML 1.0 §2).
const (
	xmlNSURI   = "http://www.w3.org/XML/1998/namespace"
	xmlnsNSURI = "http://www.w3.org/2000/xmlns/"
)

// resolveNamespaces annotates `element` (and its descendants) with
// `prefix`, `localName`, `namespace`, `space` and `lang` fields
// resolved from xmlns / xmlns:* / xml:space / xml:lang attributes
// in scope. Returns "" on success or an error code on the first
// reserved-prefix or unbound-prefix violation.
func resolveNamespaces(element map[string]any, scope map[string]string) string {
	// Pre-bind the reserved xml prefix so xml:space / xml:lang
	// qualify without an explicit declaration.
	ns := make(map[string]string, len(scope)+1)
	for k, v := range scope {
		ns[k] = v
	}
	ns["xml"] = xmlNSURI
	return resolveScope(element, xmlScope{ns: ns, space: "default", lang: ""})
}

func resolveScope(element map[string]any, scope xmlScope) string {
	local := xmlScope{
		ns:    make(map[string]string, len(scope.ns)+4),
		space: scope.space,
		lang:  scope.lang,
	}
	for k, v := range scope.ns {
		local.ns[k] = v
	}
	if attrs, ok := element["attributes"].(map[string]any); ok {
		for k, v := range attrs {
			s, _ := v.(string)
			switch {
			case k == "xmlns":
				if s == xmlNSURI || s == xmlnsNSURI {
					return "reserved_namespace"
				}
				local.ns[""] = s
			case strings.HasPrefix(k, "xmlns:"):
				prefix := k[6:]
				switch prefix {
				case "xml":
					if s != xmlNSURI {
						return "reserved_namespace"
					}
				case "xmlns":
					return "reserved_namespace"
				default:
					if s == xmlNSURI || s == xmlnsNSURI {
						return "reserved_namespace"
					}
				}
				local.ns[prefix] = s
			case k == "xml:space":
				local.space = s
			case k == "xml:lang":
				local.lang = s
			default:
				if colon := strings.Index(k, ":"); colon > 0 {
					ap := k[:colon]
					if ap != "xmlns" {
						if _, ok := local.ns[ap]; !ok {
							return "unbound_prefix"
						}
					}
				}
			}
		}
	}

	name, _ := element["name"].(string)
	if idx := strings.Index(name, ":"); idx >= 0 {
		prefix := name[:idx]
		element["prefix"] = prefix
		element["localName"] = name[idx+1:]
		if uri, ok := local.ns[prefix]; ok {
			element["namespace"] = uri
		} else {
			return "unbound_prefix"
		}
	} else {
		element["localName"] = name
		if uri, ok := local.ns[""]; ok {
			element["namespace"] = uri
		}
	}

	if local.space != "default" {
		element["space"] = local.space
	}
	if local.lang != "" {
		element["lang"] = local.lang
	}

	children, _ := element["children"].([]any)
	for _, c := range children {
		if ce, ok := c.(map[string]any); ok {
			if err := resolveScope(ce, local); err != "" {
				return err
			}
		}
	}
	return ""
}

// --- helpers ---

func advance(pnt *jsonic.Point, from, to int) {
	pnt.SI = to
	pnt.CI += to - from
}

// XML 1.0 Fifth Edition NameStartChar (§2.3 [4]): ASCII letters,
// underscore, colon and a long list of Unicode letter / ideograph
// blocks. Single-byte fast path for the common ASCII case.
func isNameStartByte(ch byte) bool {
	return (ch >= 'A' && ch <= 'Z') ||
		(ch >= 'a' && ch <= 'z') ||
		ch == '_' || ch == ':'
}

// Backwards-compat alias used by sites that only need to peek at the
// next byte (entity ref check, etc.).
func isNameStart(ch byte) bool { return isNameStartByte(ch) }

func isNameStartRune(r rune) bool {
	if r < 0x80 {
		return isNameStartByte(byte(r))
	}
	return (r >= 0xc0 && r <= 0xd6) ||
		(r >= 0xd8 && r <= 0xf6) ||
		(r >= 0xf8 && r <= 0x2ff) ||
		(r >= 0x370 && r <= 0x37d) ||
		(r >= 0x37f && r <= 0x1fff) ||
		(r >= 0x200c && r <= 0x200d) ||
		(r >= 0x2070 && r <= 0x218f) ||
		(r >= 0x2c00 && r <= 0x2fef) ||
		(r >= 0x3001 && r <= 0xd7ff) ||
		(r >= 0xf900 && r <= 0xfdcf) ||
		(r >= 0xfdf0 && r <= 0xfffd) ||
		(r >= 0x10000 && r <= 0xeffff)
}

func isNameCharByte(ch byte) bool {
	return isNameStartByte(ch) ||
		(ch >= '0' && ch <= '9') ||
		ch == '-' || ch == '.'
}

func isNameChar(ch byte) bool { return isNameCharByte(ch) }

func isNameCharRune(r rune) bool {
	if r < 0x80 {
		return isNameCharByte(byte(r))
	}
	if isNameStartRune(r) {
		return true
	}
	return r == 0xb7 ||
		(r >= 0x300 && r <= 0x36f) ||
		(r >= 0x203f && r <= 0x2040)
}

// readName reads an XML Name starting at `start` from `src`. Returns
// the name, the byte index after the name, and ok=false if the byte
// at `start` does not begin a NameStartChar (including ASCII or any
// of the Unicode ranges in §2.3 [4]).
func readName(src string, start int) (name string, end int, ok bool) {
	if start >= len(src) {
		return "", start, false
	}
	r, sz := utf8.DecodeRuneInString(src[start:])
	if r == utf8.RuneError && sz <= 1 {
		return "", start, false
	}
	if !isNameStartRune(r) {
		return "", start, false
	}
	i := start + sz
	for i < len(src) {
		r, sz := utf8.DecodeRuneInString(src[i:])
		if r == utf8.RuneError && sz <= 1 {
			break
		}
		if !isNameCharRune(r) {
			break
		}
		i += sz
	}
	return src[start:i], i, true
}

func isSpace(ch byte) bool {
	return ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r'
}

func boolPtr(b bool) *bool { return &b }

func toBool(v any, def bool) bool {
	if v == nil {
		return def
	}
	b, ok := v.(bool)
	if !ok {
		return def
	}
	return b
}

func toStringMap(v any) map[string]string {
	out := map[string]string{}
	switch m := v.(type) {
	case map[string]string:
		for k, vv := range m {
			out[k] = vv
		}
	case map[string]any:
		for k, vv := range m {
			if s, ok := vv.(string); ok {
				out[k] = s
			}
		}
	}
	return out
}
