export function localizeTimes(root: ParentNode): void {
  for (const element of root.querySelectorAll<HTMLTimeElement>(
    "time[data-local-time]",
  )) {
    const timestamp = element.dateTime;
    const date = new Date(timestamp);
    if (Number.isNaN(date.getTime())) {
      continue;
    }
    element.textContent = new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "medium",
    }).format(date);
  }
}

localizeTimes(document);
