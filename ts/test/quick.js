const { Tabnas } = require('@tabnas/parser')
const { jsonic } = require('@tabnas/jsonic')
const { Xml } = require('../dist/xml')

const xml = new Tabnas().use(jsonic).use(Xml)

console.log(
  JSON.stringify(
    xml.parse('<root><a>hello</a><b/></root>'),
    null,
    2,
  ),
)
