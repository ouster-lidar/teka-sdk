# How to Set Up a Development Host on Windows (WSL 2)

You can build Teka Apps on Windows 11. You'll install Ubuntu in WSL 2, then run the Teka CLI installer inside Ubuntu. Do this before you install the CLI.

## Set up WSL 2

1. Set your Windows network connection to **Private**, so Windows can find boxes by their .local names.
2. In PowerShell, install Ubuntu:

   ```powershell
   wsl --install -d Ubuntu-26.04
   ```

3. Turn on mirrored networking. Add these lines to %USERPROFILE%\.wslconfig. This needs Windows 11 22H2 or newer.

   ```ini
   [wsl2]
   networkingMode=mirrored
   ```

4. Shut WSL down so the change applies:

   ```powershell
   wsl --shutdown
   ```

## Install the tools

Open Ubuntu and follow [Set Up a Development Host](00_README.md) from step 01. Run those commands from the Linux filesystem, not `/mnt/c`.

## Tips

- Keep your checkout on the Linux filesystem, not under `/mnt/c`. Builds there are a lot faster.
- Can't reach a box? Check that your network is set to Private and mirrored networking is on.
