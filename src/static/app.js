document.querySelectorAll(".confirm-delete").forEach((form) => {
  form.addEventListener("submit", (event) => {
    if (!window.confirm("この項目を削除しますか？")) event.preventDefault();
  });
});
