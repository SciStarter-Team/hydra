import {LitElement, css, html} from 'lit-core.js';

customElements.define(
    "ss-rich-text",
    class SSRichText extends LitElement {
        static properties = {
            name: {type: String},
            value: {type: String}
        };

        static styles = css``;

        constructor() {
            super();
            this.name = "";
            this.value = "";
        }

        render() {
            return html`<slot></slot>`;
        }
    }
);
