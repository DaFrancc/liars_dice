<a id="readme-top"></a>
[![MIT License][license-shield]][license-url]
[![LinkedIn][linkedin-shield]][linkedin-url]



<!-- PROJECT LOGO -->
<!--
<br />
<div align="center">
  <a href="https://github.com/othneildrew/Best-README-Template">
    <img src="images/logo.png" alt="Logo" width="80" height="80">
  </a>

  <h3 align="center">Best-README-Template</h3>

  <p align="center">
    An awesome README template to jumpstart your projects!
    <br />
    <a href="https://github.com/othneildrew/Best-README-Template"><strong>Explore the docs »</strong></a>
    <br />
    <br />
    <a href="https://github.com/othneildrew/Best-README-Template">View Demo</a>
    ·
    <a href="https://github.com/othneildrew/Best-README-Template/issues/new?labels=bug&template=bug-report---.md">Report Bug</a>
    ·
    <a href="https://github.com/othneildrew/Best-README-Template/issues/new?labels=enhancement&template=feature-request---.md">Request Feature</a>
  </p>
</div>



<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
    </li>
    <li>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#credits">Credits</a></li>
  </ol>
</details>



<!-- ABOUT THE PROJECT -->
## About The Project

A simple terminal game based off of the classic liar's dice table top game. Build in rust using TCP protocols.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Prerequisites

* [Cargo](https://rust-lang.org)

### Installation

1. Clone the repo
   ```sh
   git clone https://github.com/DaFrancc/liars_dice.git
   ```
2. Build with cargo
   ```sh
   cargo build --release
   ```
3. Play! (See <a href="#usage">Usage</a> for more).

<p align="right">(<a href="#readme-top">back to top</a>)</p>


<a id="usage"></a>
<!-- USAGE EXAMPLES -->
## Usage

To start, you must first port forward port 6969 for TCP (you can also do so for UDP however this project only uses TCP).
To host a server, you launch one instance with the command
  ```sh
  liars_dice host
  ```
This will launch a server instance. You can then run a client instance on the same machine to join your server with
  ```sh
  liars_dice 127.0.0.1:6969
  ```
or you may join your friend with his IP like so
  ```sh
  liars_dice <friend IP>:6969
  ```
Clients do not need to do any more setup beyond downloading the game and joining you with the above commands.

_For more examples, please refer to the [Documentation](https://example.com)_

<p align="right">(<a href="#readme-top">back to top</a>)</p>



<!-- ROADMAP -->
## Roadmap

- [x] Set up networking
- [x] Get basic communication between client and server
- [x] Work on actual game loop
- [x] Fixing bugs
- [x] Complete base game logic
- [ ] Working on improved visuals
- [ ] Add homebrew options (wild 1's, different rules for making wagers, etc)
- [ ] Rewrite with streamlined lobby and multiplayer system

See the [open issues](https://github.com/DaFrancc/liars_dice/issues?q=sort%3Aupdated-desc+is%3Aissue+is%3Aopen) for a full list of proposed features (and known issues).

<p align="right">(<a href="#readme-top">back to top</a>)</p>



<!-- LICENSE -->
## License

Distributed under the MIT License. See `LICENSE.txt` for more information.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CREDITS -->
## Credits

Credit to [othneildrew](https://github.com/othneildrew/Best-README-Template) for this README template.
<p align="right">(<a href="#readme-top">back to top</a>)</p>


<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->
[license-shield]: https://img.shields.io/github/license/othneildrew/Best-README-Template.svg?style=for-the-badge
[license-url]: https://github.com/DaFrancc/liars_dice/blob/master/LICENSE
[linkedin-shield]: https://img.shields.io/badge/-LinkedIn-black.svg?style=for-the-badge&logo=linkedin&colorB=555
[linkedin-url]: https://www.linkedin.com/in/franciscovivas2003/
