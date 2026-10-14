export function fakeDocument() {
  function element(tag) {
    const el = {
      tag,
      children: [],
      attrs: {},
      listeners: {},
      className: '',
      _text: '',
      get textContent() {
        return el._text + el.children.map((c) => (typeof c === 'string' ? c : c.textContent)).join('');
      },
      set textContent(v) {
        el._text = String(v);
        el.children = [];
      },
      set innerHTML(_) {
        throw new Error('innerHTML must never be used');
      },
      setAttribute(k, v) {
        el.attrs[k] = String(v);
      },
      addEventListener(type, fn) {
        el.listeners[type] = fn;
      },
      click() {
        el.listeners.click?.({ preventDefault() {} });
      },
      append(...nodes) {
        el.children.push(...nodes);
      },
      replaceChildren(...nodes) {
        el._text = '';
        el.children = nodes;
      },
      find(tagName) {
        const out = [];
        const walk = (n) => {
          if (n.tag === tagName) out.push(n);
          n.children.forEach((c) => typeof c !== 'string' && walk(c));
        };
        walk(el);
        return out;
      },
    };
    return el;
  }
  return { createElement: element };
}
