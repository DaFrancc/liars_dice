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
    <li><a href="#howtoplay">Usage</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#credits">Credits</a></li>
  </ol>
</details>



<!-- ABOUT THE PROJECT -->
## About The Project

A simple terminal game based off of the classic liar's dice table top game. Build in rust using TCP protocols.
At this time, it is undergoing a major rewrite. I'm making a separate crate that will both support the same functionality as this game, as well as other types of games.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Prerequisites

* [Cargo](https://rust-lang.org)

## Installation

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
To host a server, you launch a server instance with the command
  ```sh
  liars_dice host
  ```
If you wish to play the game on the same machine, you can join your server with
  ```sh
  liars_dice 127.0.0.1:6969
  ```
You may also join your friend with his IP like so (don't share your IP with people you don't trust!)
  ```sh
  liars_dice <friend IP>:6969
  ```
Clients do not need to do any more setup beyond downloading the game and joining you with the above commands.

<!-- _For more examples, please refer to the [Documentation](https://example.com)_ -->

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## How to play

Once you've typed in the join command, you will be prompted for a username. Pick one that has not already been chosen by another player
or else you will be kicked from the server. You will have to use one of the above commands to try joining again.

You will then join the lobby where you can wait for your friends to join. There is no set player limit, however high player counts
may cause unintended side effects on the stability of the game, lead to high stress on the server, slow down the performance of the game,
lead to much longer game times, and/or cause other unexpected issues. No testing has been done to confirm nor deny these possible side effects.

Players can press 'Y' to vote to start the game or 'N' to recind their vote. All players must vote 'Y' to start the game.

Once the game has started, players will be given their random dice, a random player will start, and then continue in the order
in which players joined the game. Once the last player has finished his turn, the game will loop around and continue with the first player
to join.

When it is your turn, you will be prompted to choose your wager. You can press 'W' and 'S' to increase and decrease the face value, respectively
or you can press 'A' and 'D' to increase and decrease the quantity of the wager, respectively. The face value is between 1 and 6 and the quantity
is between 1 and 255. A valid wager must either have a higher face value with the same quantity, the same face value with a higher quantity, or
both a higher face value and a higher quantity.

Players will progressively take turns casting wagers until you decide to call someone's bluff 'L', or if you think the wager is exactly right 'K'.
If you call someone's bluff and that person wagered higher than the table, he loses a die. If you call someone's bluff and the wager was less than 
or equal to the table, you lose a die. If you think the previous wager was exactly right (no more, no less) and you were wrong, you lose a die. 
If you were right, you keep your die. You may only call a bluff or call an exact wager after the first wager has been cast. 

The game continues until there is only one person with dice left remaining.

Notes:
- If a player takes longer than 60 seconds to cast a wager, he gets kicked.
- If a player leaves in the middle of a game, unexpected behavior is to be expected and the game may crash (working on a fix).
- You don't have to call a bluff or an exact wager, you could just keep going until the game won't let you any more. The only upper limit is the
  fact that the quantities are stored in 8-bit unsigned integers. This might be updated in the future to protect the illusion of the game.
- At this moment there is an issue on some linux terminals (I've tested on Konsole and Alacritty) where input is buffered and you must press
  'ENTER' to register a keystroke. Windows terminal doesn't seem to have this issue. I'm working on a fix for this by enabling raw input,
  however it messes up the formatting and prevents users from quitting via 'CTRL+C'. This is probably not an issue for future updates
  since I will be adding actually good visuals and better control flow. If you are able to enable raw input through the console, it might work.
  I have not tested this, however.
- You are free to modify this game. I only ask that if you decide to publish your modified version that you credit me as the original author.
  I will stop updating this game after the proposed roadmap is complete and all major bugs have been squashed. My efforts will continue with
  my new proposed generic tabletop crate. I might make poker or something a bit more dynamic to test its limits.
  

<!-- _For more examples, please refer to the [Documentation](https://example.com)_ -->

<p align="right">(<a href="#readme-top">back to top</a>)</p>


<!-- ROADMAP -->
## Roadmap

- [x] Set up networking
- [x] Get basic communication between client and server
- [x] Work on actual game loop
- [x] Fixing bugs
- [x] Complete base game logic
- [ ] Add optimization options for hosts like thread caps, custom ports, player limits, passwords, better logging, etc.
- [ ] Working on improved visuals
- [ ] Add homebrew options (wild 1's, different rules for making wagers, etc)
- [ ] Rewrite with streamlined lobby and multiplayer system
- [ ] Ensure stability on Linux and Windows (Mac users are on their own)
- [ ] Make pre-compiled binaries for Linux and Windows (Mac users must build from source)

See the [open issues](https://github.com/DaFrancc/liars_dice/issues?q=sort%3Aupdated-desc+is%3Aissue+is%3Aopen) for a full list of proposed features (and known issues).

<p align="right">(<a href="#readme-top">back to top</a>)</p>



<!-- LICENSE -->
## License

Distributed under the MIT License. See `LICENSE` for more information.

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
