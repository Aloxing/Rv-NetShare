import { createApp } from "vue";
import App from "./App.vue";
import "./assets/main.css";

window.addEventListener(
  "contextmenu",
  (e) => {
    e.preventDefault();
    e.stopPropagation();
  },
  true,
);

createApp(App).mount("#app");
