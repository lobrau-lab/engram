# Engram Privacy Policy

**Draft for owner and legal review — updated October 18, 2026**
**Owner:** Tk (GitHub: `lobrau-lab`)
**Contact:** [lordlobrau@gmail.com](mailto:lordlobrau@gmail.com)

This policy describes the current Engram and Threadline application behavior. The owner does not operate an Engram account service, hosted vault, analytics backend, or cloud synchronization service. The application does accept and store content locally so that notes, imports, and conversations can work. “We do not collect your data” means the owner does not automatically receive your local vault or chat data; it does not mean the software never processes or stores user-provided content.

## 1. Who this applies to

This policy covers Engram, including its local vault workspace and the Threadline chat interface (the “Application”), when provided by Tk (the “Owner”). It does not govern third-party model servers, websites, operating systems, package managers, or payment providers. Those services have their own privacy terms.

## 2. Information kept on your device

The Application is designed to process user content on the device where it runs. Depending on the features you use, local data may include:

- Markdown notes and attachments in the vault you select;
- a local search index and other derived metadata under the vault’s `.engram` directory;
- pending Dump/import files and extraction results;
- Threadline conversations, derived memory, settings, and context-assembly records in the browser’s local storage;
- provider configuration, with the API key held in page-session memory only;
- takeover session tokens in browser and server memory, and takeover audit entries, temporary AppleScript output, and optional screen captures under `.engram/takeover/`;
- encrypted backup files you choose to create and store.

This is local storage by the Application, not collection by the Owner. The Application does not currently provide an owner-operated server that receives or stores your vault or conversation history. Data remains on your device until you or another program deletes, moves, or modifies it. Removing a conversation from Threadline does not necessarily remove notes, imported files, browser backups, or files in the vault.

Provider API keys are removed from legacy local-storage state on load and are not written back. Page-session memory is not an operating-system keychain; reloads require re-entry.

## 3. Network connections and third-party services

### Local inference

On macOS 26+ with Apple Intelligence enabled, Threadline uses Apple Foundation Models inside the Engram process. No prompt is sent over a network for that path. If you instead configure an inference server on the same device, Threadline sends prompts to that local server over loopback HTTP. This is local network traffic, not literal zero-network operation. The local inference server may have its own logs, storage, or network behavior; review that software separately.

### User-configured remote inference

Threadline can be configured with an OpenAI-compatible inference endpoint, including a server hosted by a third party. When you send a chat request to a remote endpoint, the request may contain your current message, system instructions, selected recent turns, derived conversation memory, retrieved conversation evidence, and—if enabled—retrieved Engram note excerpts. Follow-up requests may include model responses and results from local Threadline tools. The macOS `computer_use` tool is never offered to a remote endpoint. If configured, the API key is sent in the authorization header.

The remote provider receives and processes the request under its own terms and settings. The Owner does not control its retention, training, access, or deletion practices. Review that provider’s privacy and data-use terms before sending personal, confidential, or regulated information. The current prototype does not yet provide a complete per-request privacy review or data-redaction boundary for remote inference.

Model discovery and connection testing also contact the configured endpoint and may send the configured API key. These requests do not intentionally include vault notes or conversation history.

### Third-party assets

Threadline uses bundled assets and system font stacks; it does not load Google Fonts or other third-party startup assets.

### No telemetry service

The current repository does not configure an Engram-operated analytics or telemetry service. This statement does not cover user-configured inference endpoints, your operating system, browser, or tools you use to install and run the Application.

## 4. Information the Owner may receive directly

The Owner does not automatically receive your local notes, imported files, prompts, or conversation history. The Owner may receive information that you choose to send directly, for example an email or support request. That communication may contain your email address, message, and any information or files you choose to include. Do not include sensitive content unless necessary.

The repository currently has no in-app purchase or payment checkout. If paid licenses are offered later, the identity, payment, order, tax, and transaction information handled by the chosen payment provider will be described before purchase. The payment provider’s own terms will also apply. No payment processor or specific collection practice is promised by this draft.

## 5. Use, disclosure, and sale

The Owner does not sell or rent local vault or Threadline content because the Owner does not automatically receive it. The Application has no advertising or analytics feature configured in the current repository. Information you send to a third-party endpoint is governed by that provider’s terms. The Owner may disclose information received directly from you where required by law or necessary to respond to a valid legal process.

## 6. Retention and deletion

Local vault, browser, and backup data is retained on your device until you delete it. The Owner has no remote copy to delete when the Application is used without sending information directly to the Owner. If you email the Owner, the email and attachments remain in the relevant email systems; the Owner may retain correspondence as reasonably needed to respond, maintain business records, or meet legal obligations. Third-party model and payment providers set their own retention periods.

## 7. Security

The Application is under development. It includes local storage and encrypted backup features, but no software or storage method can be guaranteed secure. In particular, the current browser-stored API key is not protected by an operating-system keychain. You are responsible for securing your device, vault, browser profile, backups, and any configured inference service.

## 8. Your choices

You can choose where your vault is stored, whether to enable vault retrieval in Threadline, which inference endpoint to configure, and whether to send information to a remote provider. Do not configure a remote endpoint or enable remote use with private vault context unless you accept that provider’s handling of the information. You can delete local Threadline data using the application controls and separately delete vault files, browser data, and backups using your operating system.

The current prototype does not yet offer a complete export, data-subject request, or per-request remote-data consent workflow. Those limitations should be addressed before a public commercial release.

## 9. Changes and contact

This policy may be updated when product behavior changes. The version and effective date will be updated with material changes. Contact the Owner at [lordlobrau@gmail.com](mailto:lordlobrau@gmail.com) with privacy questions or support requests.

## 10. Separate licensing terms

This policy describes information handling; it does not grant a software license or set a price. Any one-time price (including a possible $20 license) and any proposed 10% revenue share must be defined in a separate written license or commercial schedule accepted by both parties. The existing user agreement is a draft and no checkout is currently implemented.

---

**Publication checklist:** Verify runtime network requests and third-party startup assets, implement remote-inference disclosure and safer credential storage, identify any payment processor before accepting payment, and obtain jurisdiction-specific legal review. Privacy and security claims should match observed product behavior; see the [FTC privacy and security guidance](https://www.ftc.gov/business-guidance/privacy-security).
