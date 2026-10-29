(function () {
  const frame = document.getElementById("artifact");

  async function ask(request) {
    if (request.op === "set") {
      return fetch("/_data/" + encodeURIComponent(request.key), {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request.value),
      });
    }

    return fetch("/_data");
  }

  window.addEventListener("message", async function (event) {
    const request = event.data;

    if (event.source !== frame.contentWindow || !request || request.mycelium !== "data") {
      return;
    }

    let reply = { status: 0, body: null };

    try {
      const response = await ask(request);
      const body = await response.json().catch(function () {
        return null;
      });

      reply = { status: response.status, body: body };
    } catch (error) {
      reply = { status: 0, body: null };
    }

    event.source.postMessage({ mycelium: "data-reply", id: request.id, status: reply.status, body: reply.body }, "*");
  });
})();
