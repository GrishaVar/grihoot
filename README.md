# grihoot

A basic [Kahoot](https://kahoot.it/) clone written in Rust. Launches a server which reads questions from a given text file and allows players to play the quiz via browser.
The idea is:
* Host creates a list of multiple choice questions
* Host launches the server with these questions
* Players connect to the server and chooses a nickname (important)
  * They also answer the questions, within a time limit
* Once the questions are finished, users are shown the final scoreboard
  * The player with the most correct answers feels good about themselves


## Get it running:
* Clone/download the repo
* Launch with `cargo r --release /path/to/question/file`
  * This will launch it on localhost:7878
  * See the example question file for the required structure
* Wait for users to join via browser
  * Not necessary, but prevents people from missing out on questions
* Press return to start the questions


## Details
There is an example question file to show the general structure. Basic rundown:
* Questions are seperated by two newline
* First charachter of the question is the index of the correct answer
  * Therefore, no question can have more than 10 answers
* The rest of the first line is the question
* Each following line is a possible answer

The connection between server and client is done via [WebSockets](https://en.wikipedia.org/wiki/WebSocket). This allows the client to play in their browser without constantly loading a new page. The WS implementation may be a bit janky, I wrote it myself for whatever reason.

## Possible improvements
In order of difficulty:
* Take host and port as input
* Randomise order of the questions and answers
* Make question answer time dynamic, based on length of the question
* Start quizzes via web
* Create quizzes via web as well
  * Ideally all quiz makers and quiz takers just interact with a website
* Multiple games running at once (like real kahoot)
