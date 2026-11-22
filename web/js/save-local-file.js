// Start the picker during the click, before any asynchronous image encoding or export preparation.
// A cancelled picker leaves the document and Downloads folder untouched.
export async function saveLocalFile({ filename, mime, description, data }) {
  const getBlob = async () => {
    const result = typeof data === "function" ? await data() : await data;
    if (result instanceof Blob) return result;
    if (typeof result === "string") return new Blob([result], { type: mime });
    throw new Error("Airglow could not prepare this file.");
  };
  if (typeof window.showSaveFilePicker === "function") {
    const extension = filename.match(/(\.[a-z0-9]{1,12})$/i)?.[1];
    const types = extension ? [{ description: description || mime, accept: { [mime]: [extension] } }] : undefined;
    let handle;
    try { handle = await window.showSaveFilePicker({ suggestedName: filename, types }); }
    catch (err) {
      if (err.name === "AbortError") return "cancelled";
      if (!["NotSupportedError", "SecurityError"].includes(err.name)) throw err;
    }
    if (handle) {
      const blob = await getBlob(), writer = await handle.createWritable();
      try { await writer.write(blob); await writer.close(); }
      catch (err) { try { await writer.abort(); } catch { /* already closed */ } throw err; }
      return "saved";
    }
  }
  const url = URL.createObjectURL(await getBlob());
  try {
    const link = document.createElement("a");
    link.href = url; link.download = filename; link.click();
  } finally { setTimeout(() => URL.revokeObjectURL(url), 30000); }
  return "downloaded";
}
