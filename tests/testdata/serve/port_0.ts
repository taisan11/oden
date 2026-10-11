export default {
  fetch(_req: Request) {
    return new Response("oden serve --port 0 works!");
  },
};
