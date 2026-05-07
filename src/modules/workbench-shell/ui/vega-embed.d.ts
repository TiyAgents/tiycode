declare module "vega-embed" {
  const vegaEmbed: (el: HTMLElement, spec: object, opts?: object) => Promise<{ finalize: () => void }>;
  export default vegaEmbed;
}
