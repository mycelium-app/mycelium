(function () {
  const SAVE_LABEL = "Save to Mycelium";

  document.addEventListener("webviewerloaded", function (event) {
    const options = event.detail.source.PDFViewerApplicationOptions;

    options.set("disablePreferences", true);
    options.set("enableScripting", false);
  });

  function say(message) {
    document.getElementById("myceliumSaveStatus").textContent = message;
  }

  async function save(button) {
    button.disabled = true;
    say("Saving to Mycelium.");

    try {
      const bytes = await window.PDFViewerApplication.pdfDocument.saveDocument();
      const response = await fetch("/_content", { method: "PUT", headers: { "content-type": "application/pdf" }, body: bytes });
      const result = await response.json().catch(function () { return {}; });

      if (response.ok) {
        say("Saved to Mycelium as version " + result.version + ".");
      } else {
        say(result.message || "Mycelium could not save this PDF. Try again in a minute.");
      }
    } catch (error) {
      say("Mycelium could not be reached. Check your connection, then save again.");
    } finally {
      button.disabled = false;
    }
  }

  function addSaveButton() {
    const download = document.getElementById("downloadButton");

    if (!download) {
      return;
    }

    const button = document.createElement("button");
    button.type = "button";
    button.className = "toolbarButton myceliumSave";
    button.title = SAVE_LABEL;
    button.setAttribute("aria-label", SAVE_LABEL);
    button.innerHTML = '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false"><path d="M12 3v12m0 0l-4-4m4 4l4-4M5 19h14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
    button.addEventListener("click", function () { save(button); });
    download.parentNode.insertBefore(button, download);

    const status = document.createElement("div");
    status.id = "myceliumSaveStatus";
    status.setAttribute("role", "status");
    document.body.appendChild(status);
  }

  document.addEventListener("DOMContentLoaded", addSaveButton);
})();
