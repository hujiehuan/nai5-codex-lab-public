<script setup lang="ts">
import { computed, watchEffect } from "vue";
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  darkTheme,
  useOsTheme,
  type GlobalThemeOverrides,
} from "naive-ui";
const osTheme = useOsTheme();
const isDark = computed(() => osTheme.value === "dark");
watchEffect(() => {
  document.documentElement.dataset.theme = isDark.value ? "dark" : "light";
});
const overrides = computed<GlobalThemeOverrides>(() => {
  document.documentElement.dataset.theme = isDark.value ? "dark" : "light";
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(`--${name}`).trim();
  return {
    common: {
      fontFamily: style.fontFamily,
      fontSize: "14px",
      borderRadius: "9px",
      borderRadiusSmall: "6px",
      primaryColor: color("accent"),
      primaryColorHover: color("accent-hover"),
      primaryColorPressed: color("accent-hover"),
      primaryColorSuppl: color("accent"),
      successColor: color("green"),
      warningColor: color("paid"),
      errorColor: color("danger"),
      infoColor: color("muted"),
      bodyColor: color("bg"),
      cardColor: color("panel"),
      modalColor: color("panel"),
      popoverColor: color("panel"),
      textColorBase: color("ink"),
      textColor1: color("ink"),
      textColor2: color("secondary"),
      textColor3: color("muted"),
      placeholderColor: color("muted"),
      borderColor: color("line"),
      dividerColor: color("line"),
    },
    Button: { heightSmall: "30px" },
  };
});
</script>
<template>
  <NConfigProvider
    :theme="isDark ? darkTheme : null"
    :theme-overrides="overrides"
    ><NMessageProvider
      ><NDialogProvider><slot /></NDialogProvider></NMessageProvider
  ></NConfigProvider>
</template>
