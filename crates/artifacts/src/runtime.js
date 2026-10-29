(function () {
  const waiting = new Map();
  let counter = 0;

  window.addEventListener("message", function (event) {
    const reply = event.data;

    if (event.source !== window.parent || !reply || reply.mycelium !== "data-reply") {
      return;
    }

    const settle = waiting.get(reply.id);
    waiting.delete(reply.id);

    if (settle) {
      settle(reply);
    }
  });

  function ask(request) {
    return new Promise(function (settle) {
      counter += 1;
      waiting.set(counter, settle);
      window.parent.postMessage(Object.assign({ mycelium: "data", id: counter }, request), "*");
    }).then(function (reply) {
      if (reply.status < 200 || reply.status > 299) {
        const message = reply.body && reply.body.message ? reply.body.message : "The data store could not be reached. Try again in a minute.";

        throw new Error(message);
      }

      return reply.body;
    });
  }

  window.mycelium = {
    data: {
      list: function () {
        return ask({ op: "list" });
      },
      set: function (key, value) {
        return ask({ op: "set", key: key, value: value });
      },
    },
  };
})();
