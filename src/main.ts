import { createApp } from "vue";
import App from "./App.vue";
import "./assets/main.css";
import { initTheme } from "./utils/theme";

initTheme();

window.addEventListener(
  "contextmenu",
  (e) => {
    e.preventDefault();
    e.stopPropagation();
  },
  true,
);

createApp(App).mount("#app");
