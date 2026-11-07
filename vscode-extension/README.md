# Voldiq for VS Code

Analyze a local Windows folder with the Voldiq CLI. The **Voldiq** output channel shows the saved scan, logical and known allocated sizes, coverage and up to 20 of its largest files. It respects the folder scope and limits of the connection. A scan records a snapshot in Voldiq.

## Set up a connection

1. Install the `.vsix` with **Extensions: Install from VSIX…** in VS Code. Use a trusted local Windows window.
2. Put a trusted `voldiq-cli.exe` from the corresponding Voldiq build in a location outside the open workspace. The CLI is a separate executable supplied with the Voldiq build.
3. In Voldiq, open **Settings → Agents and permissions**. Create a dedicated connection with **Scan** and its required **Metadata** access. Leave content, export, planning and execution permissions off. Limit it to the folders you want to analyze.
4. Run **Voldiq: Connect to Voldiq**. Choose the CLI, the default desktop state or an existing portable state folder, and paste the credential when asked. VS Code stores the connection in SecretStorage. If you update the CLI, run the connect command again to confirm the new executable.

Right-click a folder in Explorer and choose **Analyze Folder**. You can also run **Voldiq: Analyze Folder** from the Command Palette to pick a folder. Cancel in the progress notification to request cancellation from Voldiq. A scan can finish and save its snapshot just before cancellation arrives.

**Voldiq: Forget Connection** removes the connection from VS Code. To revoke the credential itself, use **Agents and permissions** in Voldiq.

The adapter invokes the existing CLI locally. Its commands cover scanning and metadata. It runs no terminal commands and opens no file contents. Remote, virtual and untrusted workspaces are outside its scope.
