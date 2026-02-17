# Jaeger Screenshot Procedure

1. Open `http://localhost:16686/search`.
2. Select service `inspequte` and open latest trace.
3. Identify `Expand +1` and click the next sibling control exactly once:
   - `//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')]`
   - `(//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')])[1]/following-sibling::*[contains(@class,'TimelineCollapser--btn')][1]`
4. Capture a full-page screenshot and store it under `target/oss-fp/jaeger/`.

If the XPath is unavailable, click the control immediately after the first visible `Expand +` control once.
