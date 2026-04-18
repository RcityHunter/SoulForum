USE NS auth DB main;

DEFINE TABLE boards SCHEMALESS;
DEFINE FIELD name ON boards TYPE string;
DEFINE FIELD description ON boards TYPE option<string>;
DEFINE FIELD created_at ON boards TYPE datetime;
DEFINE INDEX boards_name_idx ON boards COLUMNS name UNIQUE;

DEFINE TABLE topics SCHEMALESS;
DEFINE FIELD board_id ON topics TYPE string;
DEFINE FIELD subject ON topics TYPE string;
DEFINE FIELD author ON topics TYPE string;
DEFINE FIELD created_at ON topics TYPE datetime;
DEFINE FIELD updated_at ON topics TYPE datetime;
DEFINE INDEX topics_board_idx ON topics COLUMNS board_id;

DEFINE TABLE posts SCHEMALESS;
DEFINE FIELD topic_id ON posts TYPE string;
DEFINE FIELD board_id ON posts TYPE string;
DEFINE FIELD subject ON posts TYPE string;
DEFINE FIELD body ON posts TYPE string;
DEFINE FIELD author ON posts TYPE string;
DEFINE FIELD created_at ON posts TYPE datetime;
DEFINE INDEX posts_topic_idx ON posts COLUMNS topic_id;
DEFINE INDEX posts_board_idx ON posts COLUMNS board_id;

CREATE boards CONTENT {
    name: "General",
    description: "General discussion",
    created_at: time::now()
};
