import { get } from "./core.js";

let request;
export function lookbookCatalog() {
  return request ||= get("/data/lookbook.json").catch(error => { request=null; throw error; });
}
